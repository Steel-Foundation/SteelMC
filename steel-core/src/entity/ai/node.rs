//! Vanilla pathfinding node primitives.

use glam::DVec3;
use rustc_hash::FxHashMap;
use steel_utils::BlockPos;

use crate::entity::ai::path::PathType;

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    hash: i32,
    pub heap_idx: i32,
    pub g: f32,
    pub h: f32,
    pub f: f32,
    pub came_from: Option<i32>,
    pub closed: bool,
    pub walked_distance: f32,
    pub cost_malus: f32,
    pub path_type: PathType,
}

impl Node {
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self {
            x,
            y,
            z,
            hash: Self::create_hash(x, y, z),
            heap_idx: -1,
            g: 0.0,
            h: 0.0,
            f: 0.0,
            came_from: None,
            closed: false,
            walked_distance: 0.0,
            cost_malus: 0.0,
            path_type: PathType::Blocked,
        }
    }

    #[must_use]
    pub const fn clone_and_move(&self, x: i32, y: i32, z: i32) -> Self {
        let mut node = Self::new(x, y, z);
        node.heap_idx = self.heap_idx;
        node.g = self.g;
        node.h = self.h;
        node.f = self.f;
        node.came_from = self.came_from;
        node.closed = self.closed;
        node.walked_distance = self.walked_distance;
        node.cost_malus = self.cost_malus;
        node.path_type = self.path_type;
        node
    }

    #[must_use]
    pub const fn create_hash(x: i32, y: i32, z: i32) -> i32 {
        let mut hash =
            ((y as u32) & 0xff) | (((x as u32) & 32_767) << 8) | (((z as u32) & 32_767) << 24);
        if x < 0 {
            hash |= 0x8000_0000;
        }
        if z < 0 {
            hash |= 32_768;
        }
        hash as i32
    }

    #[must_use]
    pub const fn hash(&self) -> i32 {
        self.hash
    }

    #[must_use]
    pub fn distance_to(&self, to: &Self) -> f32 {
        let xd = (to.x - self.x) as f32;
        let yd = (to.y - self.y) as f32;
        let zd = (to.z - self.z) as f32;
        xd.mul_add(xd, yd.mul_add(yd, zd * zd)).sqrt()
    }

    #[must_use]
    pub fn distance_to_pos(&self, pos: BlockPos) -> f32 {
        let xd = (pos.x() - self.x) as f32;
        let yd = (pos.y() - self.y) as f32;
        let zd = (pos.z() - self.z) as f32;
        xd.mul_add(xd, yd.mul_add(yd, zd * zd)).sqrt()
    }

    #[must_use]
    pub fn distance_manhattan_to_pos(&self, pos: BlockPos) -> f32 {
        (pos.x() - self.x).abs() as f32
            + (pos.y() - self.y).abs() as f32
            + (pos.z() - self.z).abs() as f32
    }

    #[must_use]
    pub fn distance_to_sqr(&self, to: &Self) -> f32 {
        let xd = (to.x - self.x) as f32;
        let yd = (to.y - self.y) as f32;
        let zd = (to.z - self.z) as f32;
        xd.mul_add(xd, yd.mul_add(yd, zd * zd))
    }

    #[must_use]
    pub fn distance_manhattan(&self, to: &Self) -> f32 {
        (to.x - self.x).abs() as f32 + (to.y - self.y).abs() as f32 + (to.z - self.z).abs() as f32
    }

    #[must_use]
    pub const fn as_block_pos(&self) -> BlockPos {
        BlockPos::new(self.x, self.y, self.z)
    }

    #[must_use]
    pub fn as_vec3(&self) -> DVec3 {
        DVec3::new(f64::from(self.x), f64::from(self.y), f64::from(self.z))
    }

    #[must_use]
    pub const fn in_open_set(&self) -> bool {
        self.heap_idx >= 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    node: Node,
    best_heuristic: f32,
    best_node: Option<i32>,
    reached: bool,
}

impl Target {
    #[must_use]
    pub const fn new(node: Node) -> Self {
        Self {
            node,
            best_heuristic: f32::MAX,
            best_node: None,
            reached: false,
        }
    }

    #[must_use]
    pub const fn node(&self) -> &Node {
        &self.node
    }

    pub fn update_best(&mut self, heuristic: f32, node: &Node) {
        if heuristic < self.best_heuristic {
            self.best_heuristic = heuristic;
            self.best_node = Some(node.hash());
        }
    }

    #[must_use]
    pub const fn best_node(&self) -> Option<i32> {
        self.best_node
    }

    pub const fn set_reached(&mut self) {
        self.reached = true;
    }

    #[must_use]
    pub const fn is_reached(&self) -> bool {
        self.reached
    }
}

#[derive(Debug, Default, Clone)]
pub struct NodeStore {
    nodes: FxHashMap<i32, Node>,
}

impl NodeStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    pub fn reset_search_state(&mut self) {
        for node in self.nodes.values_mut() {
            node.heap_idx = -1;
            node.g = 0.0;
            node.h = 0.0;
            node.f = 0.0;
            node.came_from = None;
            node.closed = false;
            node.walked_distance = 0.0;
        }
    }

    pub fn get_node(&mut self, x: i32, y: i32, z: i32) -> &mut Node {
        let hash = Node::create_hash(x, y, z);
        self.nodes.entry(hash).or_insert_with(|| Node::new(x, y, z))
    }

    #[must_use]
    pub fn get(&self, hash: i32) -> Option<&Node> {
        self.nodes.get(&hash)
    }

    pub fn get_mut(&mut self, hash: i32) -> Option<&mut Node> {
        self.nodes.get_mut(&hash)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeHeap {
    heap: Vec<i32>,
}

impl NodeHeap {
    #[must_use]
    pub const fn new() -> Self {
        Self { heap: Vec::new() }
    }

    pub fn clear(&mut self, nodes: &mut NodeStore) {
        for hash in self.heap.drain(..) {
            if let Some(node) = nodes.get_mut(hash) {
                node.heap_idx = -1;
            }
        }
    }

    #[must_use]
    pub fn peek(&self) -> Option<i32> {
        self.heap.first().copied()
    }

    pub fn insert(&mut self, nodes: &mut NodeStore, hash: i32) -> bool {
        let Some(node) = nodes.get(hash) else {
            return false;
        };
        if node.in_open_set() {
            return false;
        }

        self.heap.push(hash);
        let index = self.heap.len() - 1;
        Self::set_heap_idx(nodes, hash, index) && self.up_heap(nodes, index)
    }

    pub fn pop(&mut self, nodes: &mut NodeStore) -> Option<i32> {
        let popped = self.heap.first().copied()?;
        let last = self.heap.pop()?;
        if !self.heap.is_empty() {
            self.heap[0] = last;
            if !Self::set_heap_idx(nodes, last, 0) || !self.down_heap(nodes, 0) {
                return None;
            }
        }

        Self::set_heap_idx_to_removed(nodes, popped);
        Some(popped)
    }

    pub fn change_cost(&mut self, nodes: &mut NodeStore, hash: i32, new_cost: f32) -> bool {
        let Some(node) = nodes.get_mut(hash) else {
            return false;
        };
        let old_cost = node.f;
        let heap_idx = node.heap_idx;
        node.f = new_cost;
        if heap_idx < 0 {
            return false;
        }

        let index = heap_idx as usize;
        if self.heap.get(index).copied() != Some(hash) {
            return false;
        }

        if new_cost < old_cost {
            self.up_heap(nodes, index)
        } else {
            self.down_heap(nodes, index)
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.heap.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    fn up_heap(&mut self, nodes: &mut NodeStore, mut index: usize) -> bool {
        let Some(node_hash) = self.heap.get(index).copied() else {
            return false;
        };
        let Some(cost) = nodes.get(node_hash).map(|node| node.f) else {
            return false;
        };

        while index > 0 {
            let parent_index = (index - 1) >> 1;
            let Some(parent_hash) = self.heap.get(parent_index).copied() else {
                return false;
            };
            let Some(parent_cost) = nodes.get(parent_hash).map(|node| node.f) else {
                return false;
            };
            if cost >= parent_cost {
                break;
            }

            self.heap[index] = parent_hash;
            if !Self::set_heap_idx(nodes, parent_hash, index) {
                return false;
            }
            index = parent_index;
        }

        self.heap[index] = node_hash;
        Self::set_heap_idx(nodes, node_hash, index)
    }

    fn down_heap(&mut self, nodes: &mut NodeStore, mut index: usize) -> bool {
        let Some(node_hash) = self.heap.get(index).copied() else {
            return false;
        };
        let Some(cost) = nodes.get(node_hash).map(|node| node.f) else {
            return false;
        };

        loop {
            let left_index = 1 + (index << 1);
            let right_index = left_index + 1;
            if left_index >= self.heap.len() {
                break;
            }

            let Some(left_hash) = self.heap.get(left_index).copied() else {
                return false;
            };
            let Some(left_cost) = nodes.get(left_hash).map(|node| node.f) else {
                return false;
            };
            let right = self
                .heap
                .get(right_index)
                .and_then(|hash| nodes.get(*hash).map(|node| (*hash, node.f)));

            let (child_index, child_hash, child_cost) = match right {
                Some((right_hash, right_cost)) if right_cost <= left_cost => {
                    (right_index, right_hash, right_cost)
                }
                _ => (left_index, left_hash, left_cost),
            };

            if child_cost >= cost {
                break;
            }

            self.heap[index] = child_hash;
            if !Self::set_heap_idx(nodes, child_hash, index) {
                return false;
            }
            index = child_index;
        }

        self.heap[index] = node_hash;
        Self::set_heap_idx(nodes, node_hash, index)
    }

    fn set_heap_idx(nodes: &mut NodeStore, hash: i32, index: usize) -> bool {
        let Ok(heap_idx) = i32::try_from(index) else {
            return false;
        };
        let Some(node) = nodes.get_mut(hash) else {
            return false;
        };
        node.heap_idx = heap_idx;
        true
    }

    fn set_heap_idx_to_removed(nodes: &mut NodeStore, hash: i32) {
        if let Some(node) = nodes.get_mut(hash) {
            node.heap_idx = -1;
        }
    }
}

impl Default for NodeHeap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Node, NodeHeap, NodeStore, Target};
    use crate::entity::ai::path::PathType;

    #[test]
    fn node_hash_matches_vanilla_packing_shape() {
        assert_eq!(Node::create_hash(0, 0, 0), 0);
        assert_eq!(Node::create_hash(1, 64, 2), 0x0200_0140);
        assert_eq!(Node::create_hash(-1, 0, 0), 0x807f_ff00_u32 as i32);
        assert_eq!(Node::create_hash(0, 0, -1), 0xff00_8000_u32 as i32);
    }

    #[test]
    fn node_store_reuses_nodes_by_vanilla_hash() {
        let mut store = NodeStore::new();
        let hash = store.get_node(1, 64, 2).hash();
        store.get_node(1, 64, 2).cost_malus = 4.0;

        assert_eq!(store.len(), 1);
        assert_eq!(store.get(hash).map(|node| node.cost_malus), Some(4.0));
    }

    #[test]
    fn node_store_resets_search_state_without_losing_path_type_cost() {
        let mut store = NodeStore::new();
        let hash = store.get_node(1, 64, 2).hash();
        let Some(node) = store.get_mut(hash) else {
            panic!("node should exist");
        };
        node.heap_idx = 3;
        node.g = 1.0;
        node.h = 2.0;
        node.f = 3.0;
        node.came_from = Some(Node::create_hash(0, 64, 2));
        node.closed = true;
        node.walked_distance = 4.0;
        node.cost_malus = 8.0;
        node.path_type = PathType::Water;

        store.reset_search_state();

        let Some(node) = store.get(hash) else {
            panic!("node should still exist");
        };
        assert_eq!(node.heap_idx, -1);
        assert_eq!(node.g.to_bits(), 0.0_f32.to_bits());
        assert_eq!(node.h.to_bits(), 0.0_f32.to_bits());
        assert_eq!(node.f.to_bits(), 0.0_f32.to_bits());
        assert_eq!(node.came_from, None);
        assert!(!node.closed);
        assert_eq!(node.walked_distance.to_bits(), 0.0_f32.to_bits());
        assert_eq!(node.cost_malus.to_bits(), 8.0_f32.to_bits());
        assert_eq!(node.path_type, PathType::Water);
    }

    #[test]
    fn node_heap_pops_lowest_f_cost_first() {
        let mut store = NodeStore::new();
        let high = node_with_cost(&mut store, 0, 64, 0, 5.0);
        let low = node_with_cost(&mut store, 1, 64, 0, 1.0);
        let middle = node_with_cost(&mut store, 2, 64, 0, 3.0);
        let mut heap = NodeHeap::new();

        assert!(heap.insert(&mut store, high));
        assert!(heap.insert(&mut store, low));
        assert!(heap.insert(&mut store, middle));

        assert_eq!(heap.peek(), Some(low));
        assert_eq!(heap.pop(&mut store), Some(low));
        assert_eq!(store.get(low).map(|node| node.heap_idx), Some(-1));
        assert_eq!(heap.pop(&mut store), Some(middle));
        assert_eq!(heap.pop(&mut store), Some(high));
        assert!(heap.is_empty());
    }

    #[test]
    fn node_heap_change_cost_reorders_existing_node() {
        let mut store = NodeStore::new();
        let high = node_with_cost(&mut store, 0, 64, 0, 5.0);
        let low = node_with_cost(&mut store, 1, 64, 0, 1.0);
        let mut heap = NodeHeap::new();

        assert!(heap.insert(&mut store, high));
        assert!(heap.insert(&mut store, low));
        assert_eq!(heap.peek(), Some(low));

        assert!(heap.change_cost(&mut store, high, 0.5));

        assert_eq!(heap.peek(), Some(high));
        assert_eq!(heap.pop(&mut store), Some(high));
    }

    #[test]
    fn target_tracks_best_node_hash() {
        let from = Node::new(0, 64, 0);
        let better = Node::new(1, 64, 0);
        let worse = Node::new(4, 64, 0);
        let mut target = Target::new(Node::new(2, 64, 0));

        target.update_best(10.0, &worse);
        target.update_best(1.0, &better);
        target.update_best(5.0, &from);

        assert_eq!(target.best_node(), Some(better.hash()));
        assert!(!target.is_reached());
        target.set_reached();
        assert!(target.is_reached());
    }

    fn node_with_cost(store: &mut NodeStore, x: i32, y: i32, z: i32, cost: f32) -> i32 {
        let node = store.get_node(x, y, z);
        node.f = cost;
        node.hash()
    }
}
