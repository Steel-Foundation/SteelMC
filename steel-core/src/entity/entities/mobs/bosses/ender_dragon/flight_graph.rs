//! The dragon's fixed 24-node flight graph and the A* search it flies over.
//!
//! Mirrors the graph half of vanilla `EnderDragon`: the lazy node layout and
//! `nodeAdjacency` table built inside `findClosestNode()`, plus
//! `findClosestNode(double, double, double)`, `findPath` and `reconstructPath`.

use std::f32::consts::PI;
use std::f64::consts::PI as PI_F64;

use glam::DVec3;
use steel_math::trig;
use steel_utils::BlockPos;

use crate::chunk::heightmap::HeightmapType;
use crate::entity::ai::node::Node;
use crate::entity::ai::path::Path;
use crate::world::World;

/// Number of nodes in the flight graph. Vanilla hardcodes 24 at every use.
pub const NODE_COUNT: usize = 24;

/// Nodes in the radius-60 outer ring. That is also the index of the first middle-ring
/// node and the lowest node a restricted search may use, and the modulus the holding
/// pattern wraps its targets into.
pub(super) const OUTER_RING_NODES: usize = 12;
/// First index of the radius-20 ring.
const INNER_RING_START: usize = 20;

/// Floor for every node's height.
const MINIMUM_NODE_Y: i32 = 73;
/// Vanilla's initial `closestDist`, in squared blocks.
const MAX_NODE_DISTANCE: f32 = 10_000.0;

/// Vanilla writes these as `(float)(Math.PI / n)`, an `f64` division rounded to
/// `f32`, and the angle arithmetic that follows is `float` throughout.
const FRAC_PI_12: f32 = (PI_F64 / 12.0) as f32;
const FRAC_PI_8: f32 = (PI_F64 / 8.0) as f32;
const FRAC_PI_4: f32 = (PI_F64 / 4.0) as f32;

/// Which nodes each node connects to, as a bitmask over node indices.
///
/// Copied verbatim from vanilla `EnderDragon.findClosestNode`, where the values are
/// equally magic.
const NODE_ADJACENCY: [i32; NODE_COUNT] = [
    6146, 8197, 8202, 16404, 32808, 32848, 65696, 131_392, 131_712, 263_424, 526_848, 525_313,
    1_581_057, 3_166_214, 2_138_120, 6_373_424, 4_358_208, 12_910_976, 9_044_480, 9_706_496,
    15_216_640, 13_688_832, 11_763_712, 8_257_536,
];

/// Vanilla's `2.0F * ((float)-Math.PI + step * index)`.
fn ring_angle(step: f32, index: usize) -> f32 {
    2.0 * (-PI + step * index as f32)
}

/// The lowest node index a search may use.
///
/// Vanilla drops the outer ring whenever there is no fight or every crystal is gone,
/// which pulls the dragon in towards the podium as the fight winds down.
const fn first_searchable_node(crystals: Option<i32>) -> usize {
    match crystals {
        Some(alive) if alive != 0 => 0,
        _ => OUTER_RING_NODES,
    }
}

/// Per-node A* scratch state.
///
/// Vanilla keeps these six fields on `Node` itself and resets all of them at the top
/// of every search. Steel's [`Node`] is shared with the mob pathfinder, where
/// `came_from` holds a node *hash* rather than an index, so storing indices there
/// would compile while quietly overloading the field. Keeping the state here also
/// leaves the node positions immutable, so a search needs only a `&self` borrow.
#[derive(Debug, Clone, Copy)]
struct SearchState {
    g: f32,
    h: f32,
    f: f32,
    came_from: Option<u8>,
    closed: bool,
    /// Position in the open-set heap, or `-1` when not queued. Vanilla `heapIdx`.
    heap_idx: i32,
}

impl SearchState {
    const EMPTY: Self = Self {
        g: 0.0,
        h: 0.0,
        f: 0.0,
        came_from: None,
        closed: false,
        heap_idx: -1,
    };

    /// Mirrors vanilla `Node.inOpenSet`.
    const fn in_open_set(self) -> bool {
        self.heap_idx >= 0
    }
}

/// Vanilla `BinaryHeap`, keyed by node index rather than by `Node` reference.
///
/// Only the operations `findPath` actually reaches are ported; `remove`, `peek`,
/// `size` and `getHeap` are unreachable from it. The backing array is fixed because
/// `insert` refuses a node that is already queued, so the heap can never hold more
/// than [`NODE_COUNT`] entries and vanilla's growth path is dead here.
///
/// Every comparison is a strict `<`, which is what makes ties deterministic: an
/// equal-cost newcomer never displaces a seated node, and [`Self::down_heap`] prefers
/// the **right** child when the two children tie. Ties are common, because the three rings
/// are symmetric, and a different pop order yields an equally short but visibly
/// different flight path.
struct NodeHeap {
    heap: [u8; NODE_COUNT],
    size: usize,
}

impl NodeHeap {
    const fn new() -> Self {
        Self {
            heap: [0; NODE_COUNT],
            size: 0,
        }
    }

    fn cost(state: &[SearchState; NODE_COUNT], node: u8) -> f32 {
        state[usize::from(node)].f
    }

    fn set_heap_idx(state: &mut [SearchState; NODE_COUNT], node: u8, index: usize) {
        state[usize::from(node)].heap_idx = index as i32;
    }

    fn insert(&mut self, state: &mut [SearchState; NODE_COUNT], index: usize) {
        debug_assert!(
            !state[index].in_open_set(),
            "node {index} is already in the open set"
        );

        self.heap[self.size] = index as u8;
        Self::set_heap_idx(state, index as u8, self.size);
        self.size += 1;
        self.up_heap(state, self.size - 1);
    }

    fn pop(&mut self, state: &mut [SearchState; NODE_COUNT]) -> Option<usize> {
        if self.size == 0 {
            return None;
        }

        let popped = usize::from(self.heap[0]);
        self.size -= 1;
        self.heap[0] = self.heap[self.size];
        if self.size > 0 {
            self.down_heap(state, 0);
        }

        state[popped].heap_idx = -1;
        Some(popped)
    }

    fn change_cost(&mut self, state: &mut [SearchState; NODE_COUNT], index: usize, new_cost: f32) {
        let old_cost = state[index].f;
        state[index].f = new_cost;

        // Vanilla indexes with `heapIdx` unguarded and would throw on a detached node.
        // `findPath` only ever calls this under `inOpenSet`, so the guard is a
        // formality that keeps a future caller from indexing out of bounds.
        let Ok(heap_index) = usize::try_from(state[index].heap_idx) else {
            return;
        };

        if new_cost < old_cost {
            self.up_heap(state, heap_index);
        } else {
            self.down_heap(state, heap_index);
        }
    }

    fn up_heap(&mut self, state: &mut [SearchState; NODE_COUNT], mut index: usize) {
        let node = self.heap[index];
        let cost = Self::cost(state, node);

        while index > 0 {
            let parent_index = (index - 1) >> 1;
            let parent = self.heap[parent_index];
            if cost >= Self::cost(state, parent) {
                break;
            }

            self.heap[index] = parent;
            Self::set_heap_idx(state, parent, index);
            index = parent_index;
        }

        self.heap[index] = node;
        Self::set_heap_idx(state, node, index);
    }

    fn down_heap(&mut self, state: &mut [SearchState; NODE_COUNT], mut index: usize) {
        let node = self.heap[index];
        let cost = Self::cost(state, node);

        loop {
            let left_index = 1 + (index << 1);
            if left_index >= self.size {
                break;
            }

            let right_index = left_index + 1;
            let left = self.heap[left_index];
            let left_cost = Self::cost(state, left);

            // Vanilla models a missing right child as `+inf` and takes the left child
            // only when it is *strictly* cheaper, so an exact tie goes right.
            let (child_index, child) = if right_index < self.size
                && Self::cost(state, self.heap[right_index]) <= left_cost
            {
                (right_index, self.heap[right_index])
            } else {
                (left_index, left)
            };

            if Self::cost(state, child) >= cost {
                break;
            }

            self.heap[index] = child;
            Self::set_heap_idx(state, child, index);
            index = child_index;
        }

        self.heap[index] = node;
        Self::set_heap_idx(state, node, index);
    }
}

/// The dragon's fixed 24-node flight graph.
///
/// Vanilla builds the nodes lazily inside `findClosestNode()` because it needs the
/// level's heightmap. Steel keeps that laziness on the dragon, which owns an
/// `Option<Self>`, and makes the layout an explicit constructor instead.
pub struct DragonFlightGraph {
    nodes: [Node; NODE_COUNT],
}

impl DragonFlightGraph {
    /// Samples the terrain and lays out the three rings.
    ///
    /// The rings are centered on the **world origin**, not on the dragon's fight
    /// origin. Every other position the dragon works with is fight-origin-relative, so
    /// this one is worth flagging rather than "fixing".
    ///
    /// Returns `None` when any of the 24 columns sits in a chunk that is not loaded to
    /// full status. Vanilla's `getHeightmapPos` blocking-loads instead, which Steel has
    /// no synchronous equivalent for, and [`World::level_height_at`]'s fallback would
    /// hand back the world floor: in the End that clamps every ring to
    /// [`MINIMUM_NODE_Y`], and the caller caches the graph for the dragon's whole life.
    /// Refusing lets the caller retry on a later tick, the way the portal scan and
    /// `execute ... over` already do rather than guessing.
    // TODO: Hold a chunk ticket over the arena once `EnderDragonFight` exists, so the
    // columns are guaranteed loaded and this can stop failing.
    #[must_use]
    pub fn try_build(world: &World) -> Option<Self> {
        let mut nodes = Vec::with_capacity(NODE_COUNT);
        for index in 0..NODE_COUNT {
            let (radius, y_adjustment, angle) = if index < OUTER_RING_NODES {
                (60.0_f32, 5, ring_angle(FRAC_PI_12, index))
            } else if index < INNER_RING_START {
                // Vanilla writes this as `yAdjustment += 10` over the shared 5.
                (
                    40.0_f32,
                    15,
                    ring_angle(FRAC_PI_8, index - OUTER_RING_NODES),
                )
            } else {
                (20.0_f32, 5, ring_angle(FRAC_PI_4, index - INNER_RING_START))
            };

            // `trig` is a port of `Mth.cos`/`Mth.sin`, a 65536-entry lookup table
            // rather than `f32::cos`. That distinction decides `floor` outright at
            // each ring's axis points, where the product lands on an integer.
            let x = (radius * trig::cos(f64::from(angle))).floor() as i32;
            let z = (radius * trig::sin(f64::from(angle))).floor() as i32;
            // `height_at` rather than `heightmap_pos`, because only the former
            // distinguishes "no terrain here" from "chunk not loaded". The ring stays
            // within +/-60 blocks, so the horizontal bounds check `level_height_at`
            // adds cannot trigger.
            let surface = world.height_at(HeightmapType::MotionBlockingNoLeaves, x, z)?;

            nodes.push(Node::new(x, MINIMUM_NODE_Y.max(surface + y_adjustment), z));
        }

        Some(Self {
            nodes: nodes.try_into().ok()?,
        })
    }

    /// Returns a node's position.
    ///
    /// # Panics
    ///
    /// Panics if `index` is not below [`NODE_COUNT`]. Use [`Self::get`] from anything
    /// that did not get its index out of this graph.
    #[must_use]
    pub const fn node(&self, index: usize) -> &Node {
        &self.nodes[index]
    }

    /// Returns a node's position, or `None` if `index` is out of range.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Node> {
        self.nodes.get(index)
    }

    /// Returns the graph node nearest `position`.
    ///
    /// Mirrors vanilla `EnderDragon.findClosestNode(double, double, double)`.
    /// `crystals` is `None` when there is no fight; that and a zero count both confine
    /// the search to the two inner rings.
    #[must_use]
    pub fn closest_node(&self, position: DVec3, crystals: Option<i32>) -> usize {
        let target = Node::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        );

        let mut closest_distance = MAX_NODE_DISTANCE;
        // Vanilla seeds this with 0 even when the outer ring is excluded, so a dragon
        // more than 100 blocks from every node falls back to a node it is not
        // otherwise allowed to use. Ported as-is; node 0 still has an edge inwards.
        let mut closest = 0;

        for index in first_searchable_node(crystals)..NODE_COUNT {
            let distance = self.nodes[index].distance_to_sqr(&target);
            if distance < closest_distance {
                closest_distance = distance;
                closest = index;
            }
        }

        closest
    }

    /// Searches for a path between two graph nodes. Mirrors vanilla `findPath`.
    ///
    /// `final_node` is appended to the result without being part of the graph, which
    /// is how the landing and strafing phases aim at a point of their own choosing.
    ///
    /// Returns `None` when the search made no progress at all, and also when either
    /// endpoint is out of range: this is `pub`, so a third-party phase can hand it an
    /// index the graph never produced, and refusing beats indexing past the rings.
    #[must_use]
    pub fn find_path(
        &self,
        start: usize,
        end: usize,
        final_node: Option<Node>,
        crystals: Option<i32>,
    ) -> Option<Path> {
        if start >= NODE_COUNT || end >= NODE_COUNT {
            return None;
        }

        let mut state = [SearchState::EMPTY; NODE_COUNT];
        let mut open_set = NodeHeap::new();

        state[start].h = self.nodes[start].distance_to(&self.nodes[end]);
        state[start].f = state[start].h;
        open_set.insert(&mut state, start);

        let mut closest = start;
        let minimum_node = first_searchable_node(crystals);

        while let Some(current) = open_set.pop(&mut state) {
            // Vanilla compares node *identity* here, then rescans the array for the
            // popped node's index. An index-keyed heap has both for free, and the two
            // agree because the 24 node positions are pairwise distinct.
            if current == end {
                return Some(self.reconstruct_path(&state, end, final_node));
            }

            if self.nodes[current].distance_to(&self.nodes[end])
                < self.nodes[closest].distance_to(&self.nodes[end])
            {
                closest = current;
            }
            state[current].closed = true;

            for neighbour in minimum_node..NODE_COUNT {
                if NODE_ADJACENCY[current] & (1 << neighbour) == 0 || state[neighbour].closed {
                    continue;
                }

                let tentative_g =
                    state[current].g + self.nodes[current].distance_to(&self.nodes[neighbour]);
                if state[neighbour].in_open_set() && tentative_g >= state[neighbour].g {
                    continue;
                }

                state[neighbour].came_from = Some(current as u8);
                state[neighbour].g = tentative_g;
                state[neighbour].h = self.nodes[neighbour].distance_to(&self.nodes[end]);

                let cost = state[neighbour].g + state[neighbour].h;
                if state[neighbour].in_open_set() {
                    open_set.change_cost(&mut state, neighbour, cost);
                } else {
                    state[neighbour].f = cost;
                    open_set.insert(&mut state, neighbour);
                }
            }
        }

        if closest == start {
            return None;
        }

        log::debug!("Failed to find a dragon flight path from node {start} to node {end}");
        Some(self.reconstruct_path(&state, closest, final_node))
    }

    /// Walks `came_from` back to the start. Mirrors vanilla `reconstructPath`.
    ///
    /// Vanilla chains an off-graph `finalNode` by pointing its `cameFrom` at the last
    /// graph node and reconstructing from there. Steel appends it instead, which keeps
    /// the dragon's bookkeeping out of the shared [`Node`].
    fn reconstruct_path(
        &self,
        state: &[SearchState; NODE_COUNT],
        end: usize,
        final_node: Option<Node>,
    ) -> Path {
        let target = final_node.as_ref().unwrap_or(&self.nodes[end]);
        let target = BlockPos::new(target.x, target.y, target.z);

        let mut indices = vec![end];
        let mut current = end;
        while let Some(previous) = state[current].came_from {
            current = usize::from(previous);
            indices.push(current);
        }
        indices.reverse();

        let mut nodes: Vec<Node> = indices
            .into_iter()
            .map(|index| self.nodes[index].clone())
            .collect();
        nodes.extend(final_node);

        Path::new(nodes, target, true)
    }
}

#[cfg(test)]
mod tests {
    use super::{NODE_COUNT, NodeHeap, SearchState};

    /// Builds a heap holding one node per entry of `costs`, keyed by its index.
    fn heap_with(costs: &[f32]) -> ([SearchState; NODE_COUNT], NodeHeap) {
        let mut state = [SearchState::EMPTY; NODE_COUNT];
        let mut heap = NodeHeap::new();
        for (index, &cost) in costs.iter().enumerate() {
            state[index].f = cost;
            heap.insert(&mut state, index);
        }
        (state, heap)
    }

    fn drain(state: &mut [SearchState; NODE_COUNT], heap: &mut NodeHeap) -> Vec<usize> {
        let mut popped = Vec::new();
        while let Some(index) = heap.pop(state) {
            popped.push(index);
        }
        popped
    }

    #[test]
    fn the_heap_pops_in_non_decreasing_cost_order() {
        let costs = [7.0, 3.0, 9.0, 1.0, 5.0, 2.0];
        let (mut state, mut heap) = heap_with(&costs);

        let order = drain(&mut state, &mut heap);

        assert_eq!(order, [3, 5, 1, 4, 0, 2]);
    }

    #[test]
    fn tied_children_resolve_to_the_right_one() {
        // Vanilla picks the left child only when it is *strictly* cheaper, so an exact
        // tie takes the right. After popping node 0, node 3 sinks past two tied
        // children: taking the right one leaves node 2 at the top, taking the left
        // would leave node 1. Ties are common across the three symmetric rings, and
        // the two orders give equally short but visibly different flight paths.
        let (mut state, mut heap) = heap_with(&[0.0, 1.0, 1.0, 5.0]);

        assert_eq!(heap.pop(&mut state), Some(0));
        assert_eq!(heap.pop(&mut state), Some(2));
    }

    #[test]
    fn an_equal_cost_newcomer_never_displaces_a_seated_node() {
        // Every comparison is a strict `<`, so insertion order breaks ties.
        let (mut state, mut heap) = heap_with(&[4.0, 4.0, 4.0, 4.0]);

        assert_eq!(heap.pop(&mut state), Some(0));
    }

    #[test]
    fn lowering_a_cost_moves_a_node_to_the_front() {
        let (mut state, mut heap) = heap_with(&[1.0, 2.0, 3.0]);

        heap.change_cost(&mut state, 2, 0.5);

        assert_eq!(drain(&mut state, &mut heap), [2, 0, 1]);
    }

    #[test]
    fn a_popped_node_leaves_the_open_set() {
        let (mut state, mut heap) = heap_with(&[1.0, 2.0]);

        let popped = heap.pop(&mut state).expect("the heap is not empty");

        // Vanilla clears `heapIdx` on the way out so the node can be queued again.
        assert!(!state[popped].in_open_set());
        assert!(state[1].in_open_set());
    }
}
