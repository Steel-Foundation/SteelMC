//! Vanilla-shaped A* path search.

use std::cmp::Ordering;

use steel_utils::BlockPos;

use crate::entity::ai::node::{Node, NodeHeap, Target};
use crate::entity::ai::path::{Path, PathfindingContext};
use crate::entity::ai::walk::{WalkNodeCollision, WalkNodeEvaluator};

const FUDGING: f32 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathRequest<'a> {
    pub targets: &'a [BlockPos],
    pub max_path_length: f32,
    pub reach_range: i32,
    pub max_visited_nodes_multiplier: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathFinder {
    max_visited_nodes: i32,
    open_set: NodeHeap,
}

impl PathFinder {
    #[must_use]
    pub const fn new(max_visited_nodes: i32) -> Self {
        Self {
            max_visited_nodes,
            open_set: NodeHeap::new(),
        }
    }

    #[must_use]
    pub const fn max_visited_nodes(&self) -> i32 {
        self.max_visited_nodes
    }

    pub const fn set_max_visited_nodes(&mut self, max_visited_nodes: i32) {
        self.max_visited_nodes = max_visited_nodes;
    }

    #[must_use]
    pub fn find_path(
        &mut self,
        evaluator: &mut WalkNodeEvaluator,
        context: &mut PathfindingContext<'_>,
        collision: &mut impl WalkNodeCollision,
        request: PathRequest<'_>,
    ) -> Option<Path> {
        let from = evaluator.get_start(context);
        let (from_point, mut target_entries) =
            self.prepare_search(evaluator, from, request.targets)?;
        let max_visited_nodes_adjusted =
            (self.max_visited_nodes as f32 * request.max_visited_nodes_multiplier) as i32;
        let mut reached_targets = Vec::new();
        let mut count = 0;
        while !self.open_set.is_empty() {
            count += 1;
            if count >= max_visited_nodes_adjusted {
                break;
            }

            let Some(current_hash) = self.open_set.pop(evaluator.nodes_mut()) else {
                break;
            };
            {
                let Some(current) = evaluator.node_mut(current_hash) else {
                    continue;
                };
                current.closed = true;
            }
            let Some(current_data) = evaluator.node(current_hash).map(NodeSearchData::from_node)
            else {
                continue;
            };

            for (index, target) in target_entries.iter_mut().enumerate() {
                if current_data.point.manhattan_to_node(target.target.node())
                    <= request.reach_range as f32
                {
                    target.target.set_reached();
                    if !reached_targets.contains(&index) {
                        reached_targets.push(index);
                    }
                }
            }
            if !reached_targets.is_empty() {
                break;
            }

            if current_data.point.distance_to(from_point) >= request.max_path_length {
                continue;
            }

            let neighbors = evaluator.get_neighbors(context, collision, current_hash);
            for neighbor_hash in neighbors.iter() {
                let Some(neighbor_data) =
                    evaluator.node(neighbor_hash).map(NodeSearchData::from_node)
                else {
                    continue;
                };
                let distance = current_data.point.distance_to(neighbor_data.point);
                let walked_distance = current_data.walked_distance + distance;
                let tentative_g_score = current_data.g + distance + neighbor_data.cost_malus;
                if walked_distance >= request.max_path_length {
                    continue;
                }
                if neighbor_data.in_open_set && tentative_g_score >= neighbor_data.g {
                    continue;
                }

                let h =
                    Self::get_best_h(evaluator.node(neighbor_hash)?, &mut target_entries) * FUDGING;
                let f = tentative_g_score + h;
                {
                    let Some(neighbor) = evaluator.node_mut(neighbor_hash) else {
                        continue;
                    };
                    neighbor.came_from = Some(current_hash);
                    neighbor.g = tentative_g_score;
                    neighbor.h = h;
                    neighbor.walked_distance = walked_distance;
                }

                if neighbor_data.in_open_set {
                    if !self
                        .open_set
                        .change_cost(evaluator.nodes_mut(), neighbor_hash, f)
                    {
                        return None;
                    }
                } else {
                    let Some(neighbor) = evaluator.node_mut(neighbor_hash) else {
                        continue;
                    };
                    neighbor.f = f;
                    if !self.open_set.insert(evaluator.nodes_mut(), neighbor_hash) {
                        return None;
                    }
                }
            }
        }

        if reached_targets.is_empty() {
            Self::best_unreached_path(evaluator, &target_entries)
        } else {
            Self::best_reached_path(evaluator, &target_entries, &reached_targets)
        }
    }

    fn prepare_search(
        &mut self,
        evaluator: &mut WalkNodeEvaluator,
        from: i32,
        targets: &[BlockPos],
    ) -> Option<(NodePoint, Vec<PathTarget>)> {
        if targets.is_empty() {
            return None;
        }

        evaluator.reset_search_state();
        self.open_set.clear(evaluator.nodes_mut());
        let mut target_entries = targets
            .iter()
            .copied()
            .map(PathTarget::new)
            .collect::<Vec<_>>();
        let from_point = NodePoint::from_node(evaluator.node(from)?);
        let from_h = Self::get_best_h(evaluator.node(from)?, &mut target_entries);
        {
            let from_node = evaluator.node_mut(from)?;
            from_node.g = 0.0;
            from_node.h = from_h;
            from_node.f = from_h;
            from_node.walked_distance = 0.0;
            from_node.came_from = None;
            from_node.closed = false;
        }
        if !self.open_set.insert(evaluator.nodes_mut(), from) {
            return None;
        }

        Some((from_point, target_entries))
    }

    fn get_best_h(from: &Node, targets: &mut [PathTarget]) -> f32 {
        let mut best_h = f32::MAX;
        for target in targets {
            let h = from.distance_to(target.target.node());
            target.target.update_best(h, from);
            best_h = best_h.min(h);
        }
        best_h
    }

    fn best_reached_path(
        evaluator: &WalkNodeEvaluator,
        targets: &[PathTarget],
        reached_targets: &[usize],
    ) -> Option<Path> {
        let mut best = None;
        for index in reached_targets {
            let Some(target) = targets.get(*index) else {
                continue;
            };
            let Some(path) =
                Self::reconstruct_path(evaluator, target.target.best_node(), target.pos, true)
            else {
                continue;
            };
            if best
                .as_ref()
                .is_none_or(|best_path: &Path| path.node_count() < best_path.node_count())
            {
                best = Some(path);
            }
        }
        best
    }

    fn best_unreached_path(evaluator: &WalkNodeEvaluator, targets: &[PathTarget]) -> Option<Path> {
        let mut best = None;
        for target in targets {
            let Some(path) =
                Self::reconstruct_path(evaluator, target.target.best_node(), target.pos, false)
            else {
                continue;
            };
            if best
                .as_ref()
                .is_none_or(|best_path: &Path| compare_unreached_paths(&path, best_path).is_lt())
            {
                best = Some(path);
            }
        }
        best
    }

    fn reconstruct_path(
        evaluator: &WalkNodeEvaluator,
        closest: Option<i32>,
        target: BlockPos,
        reached: bool,
    ) -> Option<Path> {
        let mut hashes = Vec::new();
        let mut current_hash = closest?;
        loop {
            hashes.push(current_hash);
            let node = evaluator.node(current_hash)?;
            let Some(came_from) = node.came_from else {
                break;
            };
            current_hash = came_from;
        }
        hashes.reverse();

        let mut nodes = Vec::with_capacity(hashes.len());
        for hash in hashes {
            nodes.push(path_node_from(evaluator.node(hash)?));
        }
        Some(Path::new(nodes, target, reached))
    }
}

impl Default for PathFinder {
    fn default() -> Self {
        Self::new(200)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PathTarget {
    target: Target,
    pos: BlockPos,
}

impl PathTarget {
    const fn new(pos: BlockPos) -> Self {
        Self {
            target: Target::new(Node::new(pos.x(), pos.y(), pos.z())),
            pos,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NodePoint {
    x: i32,
    y: i32,
    z: i32,
}

impl NodePoint {
    const fn from_node(node: &Node) -> Self {
        Self {
            x: node.x,
            y: node.y,
            z: node.z,
        }
    }

    fn distance_to(self, other: Self) -> f32 {
        let xd = (other.x - self.x) as f32;
        let yd = (other.y - self.y) as f32;
        let zd = (other.z - self.z) as f32;
        xd.mul_add(xd, yd.mul_add(yd, zd * zd)).sqrt()
    }

    fn manhattan_to_node(self, other: &Node) -> f32 {
        (other.x - self.x).abs() as f32
            + (other.y - self.y).abs() as f32
            + (other.z - self.z).abs() as f32
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct NodeSearchData {
    point: NodePoint,
    g: f32,
    walked_distance: f32,
    cost_malus: f32,
    in_open_set: bool,
}

impl NodeSearchData {
    const fn from_node(node: &Node) -> Self {
        Self {
            point: NodePoint::from_node(node),
            g: node.g,
            walked_distance: node.walked_distance,
            cost_malus: node.cost_malus,
            in_open_set: node.in_open_set(),
        }
    }
}

fn compare_unreached_paths(left: &Path, right: &Path) -> Ordering {
    left.dist_to_target()
        .total_cmp(&right.dist_to_target())
        .then_with(|| left.node_count().cmp(&right.node_count()))
}

const fn path_node_from(node: &Node) -> Node {
    let mut path_node = Node::new(node.x, node.y, node.z);
    path_node.g = node.g;
    path_node.h = node.h;
    path_node.f = node.f;
    path_node.came_from = node.came_from;
    path_node.closed = node.closed;
    path_node.walked_distance = node.walked_distance;
    path_node.cost_malus = node.cost_malus;
    path_node.path_type = node.path_type;
    path_node
}

#[cfg(test)]
mod tests {
    use std::ops::RangeInclusive;

    use steel_registry::{REGISTRY, test_support::init_test_registry, vanilla_blocks};
    use steel_utils::{BlockPos, BlockStateId, WorldAabb};

    use super::{PathFinder, PathRequest};
    use crate::behavior::init_behaviors;
    use crate::entity::ai::node::Node;
    use crate::entity::ai::path::{PathfindingContext, PathfindingMalus};
    use crate::entity::ai::walk::{MobPathSettings, WalkNodeEvaluator};
    use crate::world::LevelReader;

    struct GridLevel {
        default_state: BlockStateId,
        states: Vec<(BlockPos, BlockStateId)>,
    }

    impl GridLevel {
        fn new(default_state: BlockStateId) -> Self {
            Self {
                default_state,
                states: Vec::new(),
            }
        }

        fn with(mut self, pos: BlockPos, state: BlockStateId) -> Self {
            self.states.push((pos, state));
            self
        }
    }

    impl LevelReader for GridLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            self.states
                .iter()
                .find_map(|(state_pos, state)| (*state_pos == pos).then_some(*state))
                .unwrap_or(self.default_state)
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    #[test]
    fn pathfinder_finds_direct_walkable_path() {
        init_test_registry();
        init_behaviors();

        let level = flat_level(0..=2, -1..=1);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));
        let mut no_collision = |_aabb: WorldAabb| false;
        let mut pathfinder = PathFinder::new(128);

        let path = pathfinder.find_path(
            &mut evaluator,
            &mut context,
            &mut no_collision,
            PathRequest {
                targets: &[BlockPos::new(2, 64, 0)],
                max_path_length: 16.0,
                reach_range: 0,
                max_visited_nodes_multiplier: 1.0,
            },
        );

        let Some(path) = path else {
            panic!("path should be found");
        };
        assert!(path.can_reach());
        assert_eq!(path.node_count(), 3);
        assert_eq!(
            path.nodes()
                .iter()
                .map(Node::as_block_pos)
                .collect::<Vec<_>>(),
            vec![
                BlockPos::new(0, 64, 0),
                BlockPos::new(1, 64, 0),
                BlockPos::new(2, 64, 0)
            ]
        );
    }

    #[test]
    fn pathfinder_returns_closest_path_when_target_is_not_reached() {
        init_test_registry();
        init_behaviors();

        let level = flat_level(0..=4, -1..=1);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));
        let mut no_collision = |_aabb: WorldAabb| false;
        let mut pathfinder = PathFinder::new(128);

        let path = pathfinder.find_path(
            &mut evaluator,
            &mut context,
            &mut no_collision,
            PathRequest {
                targets: &[BlockPos::new(4, 64, 0)],
                max_path_length: 1.5,
                reach_range: 0,
                max_visited_nodes_multiplier: 1.0,
            },
        );

        let Some(path) = path else {
            panic!("closest path should be returned");
        };
        assert!(!path.can_reach());
        assert_eq!(
            path.end_node().map(Node::as_block_pos),
            Some(BlockPos::new(1, 64, 0))
        );
        assert_eq!(path.dist_to_target().to_bits(), 3.0_f32.to_bits());
    }

    fn flat_level(x_range: RangeInclusive<i32>, z_range: RangeInclusive<i32>) -> GridLevel {
        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let mut level = GridLevel::new(air);
        for x in x_range {
            for z in z_range.clone() {
                level = level.with(BlockPos::new(x, 63, z), stone);
            }
        }
        level
    }

    fn test_settings(entity_width: i32, entity_height: i32, entity_depth: i32) -> MobPathSettings {
        MobPathSettings::new(
            entity_width,
            entity_height,
            entity_depth,
            BlockPos::new(0, 64, 0),
            &PathfindingMalus::new(),
        )
    }
}
