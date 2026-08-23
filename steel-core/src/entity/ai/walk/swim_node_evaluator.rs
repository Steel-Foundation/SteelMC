use rustc_hash::FxHashMap;
use steel_math::fast_floor;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_utils::{BlockPos, Direction, WorldAabb};

use super::{
    MobPathSettings, NodeEvaluator, WalkNeighbors, WalkNodeCollision, WalkPathEvaluator,
};
use crate::entity::ai::node::{Node, NodeStore};
use crate::entity::ai::path::{PathComputationType, PathType, PathfindingContext};

const HORIZONTAL_DIRECTIONS: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

pub struct SwimNodeEvaluator {
    settings: MobPathSettings,
    allow_breaching: bool,
    nodes: NodeStore,
    path_types_by_pos_cache: FxHashMap<i64, PathType>,
}

impl SwimNodeEvaluator {
    pub fn new(settings: MobPathSettings, allow_breaching: bool) -> Self {
        Self {
            settings,
            allow_breaching,
            nodes: NodeStore::new(),
            path_types_by_pos_cache: FxHashMap::default(),
        }
    }

    fn get_cached_block_type(&mut self, context: &mut PathfindingContext<'_>, x: i32, y: i32, z: i32) -> PathType {
        let key = BlockPos::new(x, y, z).as_long();
        *self.path_types_by_pos_cache.entry(key).or_insert_with(|| {
            self.get_path_type_of_mob(context, x, y, z)
        })
    }

    fn get_path_type_of_mob(&self, context: &mut PathfindingContext<'_>, x: i32, y: i32, z: i32) -> PathType {
        let width = self.settings.entity_width();
        let height = self.settings.entity_height();
        let depth = self.settings.entity_depth();

        for dx in 0..width {
            for dy in 0..height {
                for dz in 0..depth {
                    let pos = BlockPos::new(x + dx, y + dy, z + dz);
                    let block_state = context.get_block_state(pos);
                    let fluid_state = block_state.get_fluid_state();
                    let below_pos = pos.below();
                    let below_state = context.get_block_state(below_pos);
                    if fluid_state.is_empty() && below_state.is_pathfindable(PathComputationType::Water) && block_state.is_air() {
                        return PathType::Breach;
                    }
                    if !fluid_state.is_water() {
                        return PathType::Blocked;
                    }
                }
            }
        }

        let block_state = context.get_block_state(BlockPos::new(x, y, z));
        if block_state.is_pathfindable(PathComputationType::Water) {
            PathType::Water
        } else {
            PathType::Blocked
        }
    }

    fn find_accepted_node(
        &mut self,
        context: &mut PathfindingContext<'_>,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<i32> {
        let path_type = self.get_cached_block_type(context, x, y, z);
        if self.allow_breaching && path_type == PathType::Breach || path_type == PathType::Water {
            let malus = self.settings.pathfinding_malus(path_type);
            if malus >= 0.0 {
                let node = self.nodes.get_node(x, y, z);
                node.path_type = path_type;
                node.cost_malus = node.cost_malus.max(malus);
                let fluid_state = context.get_block_state(BlockPos::new(x, y, z)).get_fluid_state();
                if fluid_state.is_empty() {
                    node.cost_malus += 8.0;
                }
                return Some(node.hash());
            }
        }
        None
    }

    fn is_node_valid(&self, node: Option<i32>) -> bool {
        node.and_then(|h| self.nodes.get(h)).is_some_and(|n| !n.closed)
    }

    fn has_malus(node: Option<i32>, nodes: &NodeStore) -> bool {
        node.and_then(|h| nodes.get(h)).is_some_and(|n| n.cost_malus >= 0.0)
    }
}

impl NodeEvaluator for SwimNodeEvaluator {
    fn reset_search_state(&mut self) {
        self.nodes.reset_search_state();
        self.path_types_by_pos_cache.clear();
    }

    fn node(&self, hash: i32) -> Option<&Node> {
        self.nodes.get(hash)
    }

    fn node_mut(&mut self, hash: i32) -> Option<&mut Node> {
        self.nodes.get_mut(hash)
    }

    fn nodes_mut(&mut self) -> &mut NodeStore {
        &mut self.nodes
    }

    fn get_start(&mut self, context: &mut PathfindingContext<'_>) -> i32 {
        let bb = self.settings.bounding_box();
        let x = fast_floor(bb.min_x());
        let y = fast_floor(bb.min_y() + 0.5);
        let z = fast_floor(bb.min_z());
        let node = self.nodes.get_node(x, y, z);
        node.hash()
    }

    fn get_neighbors(
        &mut self,
        context: &mut PathfindingContext<'_>,
        _collision: &mut impl WalkNodeCollision,
        pos_hash: i32,
    ) -> WalkNeighbors {
        let mut neighbors = WalkNeighbors::new();

        let Some(pos_node) = self.node(pos_hash) else {
            return neighbors;
        };
        let (cx, cy, cz) = (pos_node.x, pos_node.y, pos_node.z);

        let mut cardinal_nodes = [None; 4];
        for (i, dir) in HORIZONTAL_DIRECTIONS.iter().copied().enumerate() {
            let (dx, dy, dz) = dir.offset();
            let n = self.find_accepted_node(context, cx + dx, cy + dy, cz + dz);
            cardinal_nodes[i] = n;
            if self.is_node_valid(n) {
                if let Some(h) = n {
                    neighbors.push(h);
                }
            }
        }

        for i in 0..4 {
            let dir = HORIZONTAL_DIRECTIONS[i];
            let second_dir = HORIZONTAL_DIRECTIONS[(i + 1) % 4];
            let first = cardinal_nodes[i];
            let second = cardinal_nodes[(i + 1) % 4];
            if Self::has_malus(first, &self.nodes) && Self::has_malus(second, &self.nodes) {
                let (dx1, dy1, dz1) = dir.offset();
                let (dx2, dy2, dz2) = second_dir.offset();
                let n = self.find_accepted_node(
                    context,
                    cx + dx1 + dx2,
                    cy + dy1 + dy2,
                    cz + dz1 + dz2,
                );
                if self.is_node_valid(n) {
                    if let Some(h) = n {
                        neighbors.push(h);
                    }
                }
            }
        }

        neighbors
    }
}