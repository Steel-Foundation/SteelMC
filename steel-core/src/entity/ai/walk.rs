//! Vanilla walk path-type classification.

use steel_math::fast_floor;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, Direction, WorldAabb, axis::Axis};

use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext, BlockStateBehaviorExt as _};
use crate::entity::Mob;
use crate::entity::ai::node::{Node, NodeStore};
use crate::entity::ai::path::{
    PathComputationType, PathType, PathTypeSet, PathfindingContext, PathfindingMalus,
};
use crate::fluid::FluidStateExt as _;
use crate::world::LevelReader;

#[derive(Debug, Clone)]
pub struct MobPathSettings {
    entity_width: i32,
    entity_height: i32,
    entity_depth: i32,
    mob_position_vec: glam::DVec3,
    mob_position: BlockPos,
    bounding_box: WorldAabb,
    on_ground: bool,
    in_water: bool,
    can_stand_on_fluid: fn(FluidState) -> bool,
    max_up_step: f32,
    max_fall_distance: i32,
    malus: [f32; PathType::COUNT],
    can_pass_doors: bool,
    can_open_doors: bool,
    can_float: bool,
    can_walk_over_fences: bool,
}

impl MobPathSettings {
    #[must_use]
    pub fn from_mob<M: Mob + ?Sized>(mob: &M) -> Self {
        let bounding_box = mob.bounding_box();
        let mut malus = [0.0; PathType::COUNT];
        for path_type in PathType::ALL {
            malus[path_type.index()] = mob.get_pathfinding_malus(path_type);
        }

        let navigation = mob.mob_base().navigation().lock();
        let can_float = navigation.can_float();
        let can_open_doors = navigation.can_open_doors();
        let can_walk_over_fences = navigation.can_walk_over_fences();
        drop(navigation);

        Self {
            entity_width: fast_floor(bounding_box.width() + 1.0),
            entity_height: fast_floor(bounding_box.height() + 1.0),
            entity_depth: fast_floor(bounding_box.width() + 1.0),
            mob_position_vec: mob.position(),
            mob_position: mob.block_position(),
            bounding_box,
            on_ground: mob.on_ground(),
            in_water: mob.is_in_water(),
            can_stand_on_fluid: |_| false,
            max_up_step: mob.max_up_step(),
            max_fall_distance: mob.max_fall_distance(),
            malus,
            can_pass_doors: true,
            can_open_doors,
            can_float,
            can_walk_over_fences,
        }
    }

    #[must_use]
    pub fn new(
        entity_width: i32,
        entity_height: i32,
        entity_depth: i32,
        mob_position: BlockPos,
        pathfinding_malus: &PathfindingMalus,
    ) -> Self {
        let width = entity_width.max(1);
        let height = entity_height.max(1);
        let depth = entity_depth.max(1);
        let center_x = f64::from(mob_position.x()) + 0.5;
        let center_z = f64::from(mob_position.z()) + 0.5;
        let bounding_box = WorldAabb::new(
            center_x - f64::from(width) * 0.5,
            f64::from(mob_position.y()),
            center_z - f64::from(depth) * 0.5,
            center_x + f64::from(width) * 0.5,
            f64::from(mob_position.y()) + f64::from(height),
            center_z + f64::from(depth) * 0.5,
        );
        let mut malus = [0.0; PathType::COUNT];
        for path_type in PathType::ALL {
            malus[path_type.index()] = pathfinding_malus.get(path_type);
        }

        Self {
            entity_width: width,
            entity_height: height,
            entity_depth: depth,
            mob_position_vec: glam::DVec3::new(center_x, f64::from(mob_position.y()), center_z),
            mob_position,
            bounding_box,
            on_ground: true,
            in_water: false,
            can_stand_on_fluid: |_| false,
            max_up_step: 0.6,
            max_fall_distance: 3,
            malus,
            can_pass_doors: true,
            can_open_doors: false,
            can_float: false,
            can_walk_over_fences: false,
        }
    }

    #[must_use]
    pub const fn with_can_pass_doors(mut self, can_pass_doors: bool) -> Self {
        self.can_pass_doors = can_pass_doors;
        self
    }

    #[must_use]
    pub const fn with_can_open_doors(mut self, can_open_doors: bool) -> Self {
        self.can_open_doors = can_open_doors;
        self
    }

    #[must_use]
    pub const fn with_can_float(mut self, can_float: bool) -> Self {
        self.can_float = can_float;
        self
    }

    #[must_use]
    pub const fn with_can_walk_over_fences(mut self, can_walk_over_fences: bool) -> Self {
        self.can_walk_over_fences = can_walk_over_fences;
        self
    }

    #[must_use]
    pub const fn with_max_up_step(mut self, max_up_step: f32) -> Self {
        self.max_up_step = max_up_step;
        self
    }

    #[must_use]
    pub const fn with_max_fall_distance(mut self, max_fall_distance: i32) -> Self {
        self.max_fall_distance = max_fall_distance;
        self
    }

    #[must_use]
    pub const fn with_on_ground(mut self, on_ground: bool) -> Self {
        self.on_ground = on_ground;
        self
    }

    #[must_use]
    pub const fn with_in_water(mut self, in_water: bool) -> Self {
        self.in_water = in_water;
        self
    }

    #[must_use]
    pub const fn with_can_stand_on_fluid(
        mut self,
        can_stand_on_fluid: fn(FluidState) -> bool,
    ) -> Self {
        self.can_stand_on_fluid = can_stand_on_fluid;
        self
    }

    #[must_use]
    pub const fn entity_width(&self) -> i32 {
        self.entity_width
    }

    #[must_use]
    pub const fn entity_height(&self) -> i32 {
        self.entity_height
    }

    #[must_use]
    pub const fn entity_depth(&self) -> i32 {
        self.entity_depth
    }

    #[must_use]
    pub const fn mob_position(&self) -> BlockPos {
        self.mob_position
    }

    #[must_use]
    pub const fn mob_position_vec(&self) -> glam::DVec3 {
        self.mob_position_vec
    }

    #[must_use]
    pub const fn bounding_box(&self) -> WorldAabb {
        self.bounding_box
    }

    #[must_use]
    pub const fn on_ground(&self) -> bool {
        self.on_ground
    }

    #[must_use]
    pub const fn in_water(&self) -> bool {
        self.in_water
    }

    #[must_use]
    pub fn can_stand_on_fluid(&self, fluid_state: FluidState) -> bool {
        (self.can_stand_on_fluid)(fluid_state)
    }

    #[must_use]
    pub const fn max_up_step(&self) -> f32 {
        self.max_up_step
    }

    #[must_use]
    pub const fn max_fall_distance(&self) -> i32 {
        self.max_fall_distance
    }

    #[must_use]
    pub const fn pathfinding_malus(&self, path_type: PathType) -> f32 {
        self.malus[path_type.index()]
    }

    #[must_use]
    pub const fn can_pass_doors(&self) -> bool {
        self.can_pass_doors
    }

    #[must_use]
    pub const fn can_open_doors(&self) -> bool {
        self.can_open_doors
    }

    #[must_use]
    pub const fn can_float(&self) -> bool {
        self.can_float
    }

    #[must_use]
    pub const fn can_walk_over_fences(&self) -> bool {
        self.can_walk_over_fences
    }
}

#[derive(Debug, Clone)]
pub struct WalkNodeEvaluator {
    settings: MobPathSettings,
    nodes: NodeStore,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcceptedNodeRequest {
    pub pos: BlockPos,
    pub jump_size: i32,
    pub node_height: f64,
    pub travel_direction: Direction,
    pub current_path_type: PathType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkNeighbors {
    nodes: [Option<i32>; 8],
    len: usize,
}

impl WalkNeighbors {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            nodes: [None; 8],
            len: 0,
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = i32> + '_ {
        self.nodes[..self.len].iter().copied().flatten()
    }

    const fn push(&mut self, node: i32) {
        self.nodes[self.len] = Some(node);
        self.len += 1;
    }
}

impl Default for WalkNeighbors {
    fn default() -> Self {
        Self::new()
    }
}

const VANILLA_HORIZONTAL_DIRECTIONS: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

impl WalkNodeEvaluator {
    #[must_use]
    pub fn new(settings: MobPathSettings) -> Self {
        Self {
            settings,
            nodes: NodeStore::new(),
        }
    }

    #[must_use]
    pub const fn settings(&self) -> &MobPathSettings {
        &self.settings
    }

    pub fn clear_nodes(&mut self) {
        self.nodes.clear();
    }

    #[must_use]
    pub fn node(&self, hash: i32) -> Option<&Node> {
        self.nodes.get(hash)
    }

    pub(crate) fn node_mut(&mut self, hash: i32) -> Option<&mut Node> {
        self.nodes.get_mut(hash)
    }

    pub(crate) const fn nodes_mut(&mut self) -> &mut NodeStore {
        &mut self.nodes
    }

    pub(crate) fn reset_search_state(&mut self) {
        self.nodes.reset_search_state();
    }

    #[must_use]
    pub fn get_start(&mut self, context: &mut PathfindingContext<'_>) -> i32 {
        let position = self.settings.mob_position_vec();
        let mut start_y = self.settings.mob_position().y();
        let mut reusable_pos = BlockPos::containing(position.x, f64::from(start_y), position.z);
        let mut block_state = context.get_block_state(reusable_pos);

        if self
            .settings
            .can_stand_on_fluid(block_state.get_fluid_state())
        {
            while self
                .settings
                .can_stand_on_fluid(block_state.get_fluid_state())
            {
                start_y += 1;
                reusable_pos = BlockPos::containing(position.x, f64::from(start_y), position.z);
                block_state = context.get_block_state(reusable_pos);
            }
            start_y -= 1;
        } else if self.settings.can_float() && self.settings.in_water() {
            while block_state.get_fluid_state().is_water() {
                start_y += 1;
                reusable_pos = BlockPos::containing(position.x, f64::from(start_y), position.z);
                block_state = context.get_block_state(reusable_pos);
            }
            start_y -= 1;
        } else if self.settings.on_ground() {
            start_y = fast_floor(position.y + 0.5);
        } else {
            reusable_pos = BlockPos::containing(position.x, position.y + 1.0, position.z);

            while reusable_pos.y() > context.level().min_y() {
                start_y = reusable_pos.y();
                reusable_pos = reusable_pos.below();
                let below_block_state = context.get_block_state(reusable_pos);
                if !below_block_state.is_air()
                    && !below_block_state.is_pathfindable(PathComputationType::Land)
                {
                    break;
                }
            }
        }

        let start_pos = self.settings.mob_position();
        let centered_start = BlockPos::new(start_pos.x(), start_y, start_pos.z());
        if !self.can_start_at(context, centered_start)
            && let Some(corner) = self.first_startable_corner(context, start_y)
        {
            return self.get_start_node(context, corner);
        }

        self.get_start_node(context, centered_start)
    }

    #[must_use]
    pub fn get_neighbors(
        &mut self,
        context: &mut PathfindingContext<'_>,
        collision: &mut impl WalkNodeCollision,
        pos_hash: i32,
    ) -> WalkNeighbors {
        let Some(pos) = self.node(pos_hash) else {
            return WalkNeighbors::new();
        };
        let pos_x = pos.x;
        let pos_y = pos.y;
        let pos_z = pos.z;
        let pos_cost_malus = pos.cost_malus;
        let pos_block = BlockPos::new(pos_x, pos_y, pos_z);

        let path_type_above = self.get_path_type_of_mob(context, pos_x, pos_y + 1, pos_z);
        let current_path_type = self.get_path_type_of_mob(context, pos_x, pos_y, pos_z);
        let jump_size = if self.settings.pathfinding_malus(path_type_above) >= 0.0
            && current_path_type != PathType::StickyHoney
        {
            fast_floor(f64::from(self.settings.max_up_step()).max(1.0))
        } else {
            0
        };
        let pos_height = self.get_floor_level(context, pos_block);

        let mut neighbors = WalkNeighbors::new();
        let mut reusable_neighbors = [None; 4];
        for (index, direction) in VANILLA_HORIZONTAL_DIRECTIONS.iter().copied().enumerate() {
            let (step_x, _, step_z) = direction.offset();
            let node = self.find_accepted_node(
                context,
                collision,
                AcceptedNodeRequest {
                    pos: BlockPos::new(pos_x + step_x, pos_y, pos_z + step_z),
                    jump_size,
                    node_height: pos_height,
                    travel_direction: direction,
                    current_path_type,
                },
            );
            reusable_neighbors[index] = node;
            if self.is_neighbor_valid(node, pos_cost_malus)
                && let Some(node) = node
            {
                neighbors.push(node);
            }
        }

        for (index, direction) in VANILLA_HORIZONTAL_DIRECTIONS.iter().copied().enumerate() {
            let second_index = clockwise_direction_index(index);
            let second_direction = VANILLA_HORIZONTAL_DIRECTIONS[second_index];
            if !self.is_diagonal_corner_valid(
                pos_y,
                reusable_neighbors[index],
                reusable_neighbors[second_index],
            ) {
                continue;
            }

            let (step_x, _, step_z) = direction.offset();
            let (second_step_x, _, second_step_z) = second_direction.offset();
            let node = self.find_accepted_node(
                context,
                collision,
                AcceptedNodeRequest {
                    pos: BlockPos::new(
                        pos_x + step_x + second_step_x,
                        pos_y,
                        pos_z + step_z + second_step_z,
                    ),
                    jump_size,
                    node_height: pos_height,
                    travel_direction: direction,
                    current_path_type,
                },
            );
            if self.is_diagonal_node_valid(node)
                && let Some(node) = node
            {
                neighbors.push(node);
            }
        }

        neighbors
    }

    #[must_use]
    pub fn get_floor_level(&self, context: &PathfindingContext<'_>, pos: BlockPos) -> f64 {
        if self.settings.can_float() && context.get_block_state(pos).get_fluid_state().is_water() {
            return f64::from(pos.y()) + 0.5;
        }

        Self::floor_level(context.level(), pos)
    }

    #[must_use]
    pub fn floor_level(level: &dyn LevelReader, pos: BlockPos) -> f64 {
        let target = pos.offset(0, -1, 0);
        let state = level.get_block_state(target);
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        let shape =
            behavior.get_collision_shape(state, level, target, BlockCollisionContext::empty());
        f64::from(target.y())
            + if shape.is_empty() {
                0.0
            } else {
                shape.max(Axis::Y)
            }
    }

    #[must_use]
    pub fn get_path_type_of_mob(
        &self,
        context: &mut PathfindingContext<'_>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        let block_types = self.get_path_type_within_mob_bb(context, x, y, z);
        if let Some(path_type) = block_types.single() {
            return path_type;
        }

        if block_types.contains(PathType::Fence) {
            return PathType::Fence;
        }

        if block_types.contains(PathType::UnpassableRail) {
            return PathType::UnpassableRail;
        }

        let mut highest_malus_path_type = PathType::Blocked;
        let mut highest_malus = self.settings.pathfinding_malus(highest_malus_path_type);
        for path_type in block_types.iter() {
            let malus = self.settings.pathfinding_malus(path_type);
            if malus < 0.0 {
                return path_type;
            }
            if malus >= highest_malus {
                highest_malus = malus;
                highest_malus_path_type = path_type;
            }
        }

        let current_node_path_type = WalkPathEvaluator::path_type(context, x, y, z);
        if self.settings.entity_width > 1 {
            let current_is_cheaper =
                self.settings.pathfinding_malus(current_node_path_type) < highest_malus;
            let cap_due_to_cheap_node = current_is_cheaper
                && self
                    .settings
                    .pathfinding_malus(PathType::BigMobsCloseToDanger)
                    < highest_malus;
            if cap_due_to_cheap_node {
                PathType::BigMobsCloseToDanger
            } else {
                highest_malus_path_type
            }
        } else if current_node_path_type == PathType::Open
            && highest_malus_path_type != PathType::Open
            && highest_malus == 0.0
        {
            PathType::Open
        } else {
            highest_malus_path_type
        }
    }

    pub fn find_accepted_node(
        &mut self,
        context: &mut PathfindingContext<'_>,
        collision: &mut impl WalkNodeCollision,
        request: AcceptedNodeRequest,
    ) -> Option<i32> {
        let x = request.pos.x();
        let y = request.pos.y();
        let z = request.pos.z();
        let max_y_target = self.get_floor_level(context, request.pos);
        if max_y_target - request.node_height > self.mob_jump_height() {
            return None;
        }

        let path_type = self.get_path_type_of_mob(context, x, y, z);
        let path_cost = self.settings.pathfinding_malus(path_type);
        let mut best = if path_cost >= 0.0 {
            Some(self.get_node_and_update_cost_to_max(x, y, z, path_type, path_cost))
        } else {
            None
        };

        if let Some(best_hash) = best {
            let needs_collision_check =
                does_block_have_partial_collision(request.current_path_type)
                    && self
                        .node(best_hash)
                        .is_some_and(|node| node.cost_malus >= 0.0);
            if needs_collision_check && !self.can_reach_without_collision(collision, best_hash) {
                best = None;
            }
        }

        if path_type == PathType::Walkable {
            return best;
        }

        let needs_jump = best.is_none_or(|best_hash| {
            self.node(best_hash)
                .is_none_or(|node| node.cost_malus < 0.0)
        });
        if needs_jump
            && request.jump_size > 0
            && (path_type != PathType::Fence || self.settings.can_walk_over_fences())
            && path_type != PathType::UnpassableRail
            && path_type != PathType::Trapdoor
            && path_type != PathType::PowderSnow
        {
            return self.try_jump_on(context, collision, request);
        }

        if path_type == PathType::Water && !self.settings.can_float() {
            return self.try_find_first_non_water_below(context, x, y, z, best);
        }

        if path_type == PathType::Open {
            return Some(self.try_find_first_ground_node_below(context, x, y, z));
        }

        if does_block_have_partial_collision(path_type) && best.is_none() {
            return Some(self.get_closed_node(x, y, z, path_type));
        }

        best
    }

    #[must_use]
    pub fn get_path_type_within_mob_bb(
        &self,
        context: &mut PathfindingContext<'_>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathTypeSet {
        let mut block_types = PathTypeSet::new();
        let mut mob_on_rail = None;

        for dx in 0..self.settings.entity_width {
            for dy in 0..self.settings.entity_height {
                for dz in 0..self.settings.entity_depth {
                    let mut block_type =
                        WalkPathEvaluator::path_type(context, x + dx, y + dy, z + dz);
                    block_type =
                        self.adjust_path_type_for_mob(context, block_type, &mut mob_on_rail);
                    block_types.insert(block_type);
                }
            }
        }

        block_types
    }

    fn adjust_path_type_for_mob(
        &self,
        context: &mut PathfindingContext<'_>,
        block_type: PathType,
        mob_on_rail: &mut Option<bool>,
    ) -> PathType {
        if block_type == PathType::DoorWoodClosed
            && self.settings.can_open_doors
            && self.settings.can_pass_doors
        {
            return PathType::WalkableDoor;
        }

        if block_type == PathType::DoorOpen && !self.settings.can_pass_doors {
            return PathType::Blocked;
        }

        if block_type != PathType::Rail {
            return block_type;
        }

        if mob_on_rail.is_none() {
            let mob_position = self.settings.mob_position();
            *mob_on_rail = Some(
                WalkPathEvaluator::path_type(
                    context,
                    mob_position.x(),
                    mob_position.y(),
                    mob_position.z(),
                ) == PathType::Rail
                    || WalkPathEvaluator::path_type(
                        context,
                        mob_position.x(),
                        mob_position.y() - 1,
                        mob_position.z(),
                    ) == PathType::Rail,
            );
        }

        if matches!(mob_on_rail, Some(true)) {
            PathType::Rail
        } else {
            PathType::UnpassableRail
        }
    }

    fn first_startable_corner(
        &self,
        context: &mut PathfindingContext<'_>,
        start_y: i32,
    ) -> Option<BlockPos> {
        let bounding_box = self.settings.bounding_box();
        [
            BlockPos::containing(
                bounding_box.min_x(),
                f64::from(start_y),
                bounding_box.min_z(),
            ),
            BlockPos::containing(
                bounding_box.min_x(),
                f64::from(start_y),
                bounding_box.max_z(),
            ),
            BlockPos::containing(
                bounding_box.max_x(),
                f64::from(start_y),
                bounding_box.min_z(),
            ),
            BlockPos::containing(
                bounding_box.max_x(),
                f64::from(start_y),
                bounding_box.max_z(),
            ),
        ]
        .into_iter()
        .find(|pos| self.can_start_at(context, *pos))
    }

    fn get_start_node(&mut self, context: &mut PathfindingContext<'_>, pos: BlockPos) -> i32 {
        let path_type = self.get_path_type_of_mob(context, pos.x(), pos.y(), pos.z());
        let cost_malus = self.settings.pathfinding_malus(path_type);
        let node = self.nodes.get_node(pos.x(), pos.y(), pos.z());
        node.path_type = path_type;
        node.cost_malus = cost_malus;
        node.hash()
    }

    fn can_start_at(&self, context: &mut PathfindingContext<'_>, pos: BlockPos) -> bool {
        let path_type = self.get_path_type_of_mob(context, pos.x(), pos.y(), pos.z());
        path_type != PathType::Open && self.settings.pathfinding_malus(path_type) >= 0.0
    }

    fn is_neighbor_valid(&self, node: Option<i32>, current_cost_malus: f32) -> bool {
        let Some(node) = node.and_then(|hash| self.node(hash)) else {
            return false;
        };

        !node.closed && (node.cost_malus >= 0.0 || current_cost_malus < 0.0)
    }

    fn is_diagonal_corner_valid(
        &self,
        current_y: i32,
        first: Option<i32>,
        second: Option<i32>,
    ) -> bool {
        let Some(first) = first.and_then(|hash| self.node(hash)) else {
            return false;
        };
        let Some(second) = second.and_then(|hash| self.node(hash)) else {
            return false;
        };

        if first.y > current_y || second.y > current_y {
            return false;
        }
        if first.path_type == PathType::WalkableDoor || second.path_type == PathType::WalkableDoor {
            return false;
        }
        if self.settings.bounding_box().width() > 1.0
            && (first.cost_malus > 0.0 || second.cost_malus > 0.0)
        {
            return false;
        }

        let can_pass_between_fence_posts = first.path_type == PathType::Fence
            && second.path_type == PathType::Fence
            && self.settings.bounding_box().width() < 0.5;
        (first.y < current_y || first.cost_malus >= 0.0 || can_pass_between_fence_posts)
            && (second.y < current_y || second.cost_malus >= 0.0 || can_pass_between_fence_posts)
    }

    fn is_diagonal_node_valid(&self, node: Option<i32>) -> bool {
        let Some(node) = node.and_then(|hash| self.node(hash)) else {
            return false;
        };

        !node.closed && node.path_type != PathType::WalkableDoor && node.cost_malus >= 0.0
    }

    fn try_jump_on(
        &mut self,
        context: &mut PathfindingContext<'_>,
        collision: &mut impl WalkNodeCollision,
        request: AcceptedNodeRequest,
    ) -> Option<i32> {
        let x = request.pos.x();
        let y = request.pos.y();
        let z = request.pos.z();
        let node_above = self.find_accepted_node(
            context,
            collision,
            AcceptedNodeRequest {
                pos: request.pos.offset(0, 1, 0),
                jump_size: request.jump_size - 1,
                ..request
            },
        )?;

        if self.settings.bounding_box().width() >= 1.0 {
            return Some(node_above);
        }

        let node = self.node(node_above)?;
        if node.path_type != PathType::Open && node.path_type != PathType::Walkable {
            return Some(node_above);
        }

        let (step_x, _, step_z) = request.travel_direction.offset();
        let center_x = f64::from(x - step_x) + 0.5;
        let center_z = f64::from(z - step_z) + 0.5;
        let half_width = self.settings.bounding_box().width() / 2.0;
        let min_y = self.get_floor_level(
            context,
            BlockPos::new(fast_floor(center_x), y + 1, fast_floor(center_z)),
        ) + 0.001;
        let max_y = self.get_floor_level(context, BlockPos::new(node.x, node.y, node.z))
            + self.settings.bounding_box().height()
            - 0.002;
        let collision_box = WorldAabb::new(
            center_x - half_width,
            min_y,
            center_z - half_width,
            center_x + half_width,
            max_y,
            center_z + half_width,
        );

        if collision.has_collision(collision_box) {
            None
        } else {
            Some(node_above)
        }
    }

    fn try_find_first_non_water_below(
        &mut self,
        context: &mut PathfindingContext<'_>,
        x: i32,
        mut y: i32,
        z: i32,
        mut best: Option<i32>,
    ) -> Option<i32> {
        y -= 1;

        while y > context.level().min_y() {
            let path_type = self.get_path_type_of_mob(context, x, y, z);
            if path_type != PathType::Water {
                return best;
            }

            let path_cost = self.settings.pathfinding_malus(path_type);
            best = Some(self.get_node_and_update_cost_to_max(x, y, z, path_type, path_cost));
            y -= 1;
        }

        best
    }

    fn try_find_first_ground_node_below(
        &mut self,
        context: &mut PathfindingContext<'_>,
        x: i32,
        y: i32,
        z: i32,
    ) -> i32 {
        for current_y in (context.level().min_y()..y).rev() {
            if y - current_y > self.settings.max_fall_distance() {
                return self.get_blocked_node(x, current_y, z);
            }

            let path_type = self.get_path_type_of_mob(context, x, current_y, z);
            let path_cost = self.settings.pathfinding_malus(path_type);
            if path_type != PathType::Open {
                if path_cost >= 0.0 {
                    return self
                        .get_node_and_update_cost_to_max(x, current_y, z, path_type, path_cost);
                }

                return self.get_blocked_node(x, current_y, z);
            }
        }

        self.get_blocked_node(x, y, z)
    }

    fn can_reach_without_collision(
        &self,
        collision: &mut impl WalkNodeCollision,
        target: i32,
    ) -> bool {
        let Some(node) = self.node(target) else {
            return false;
        };
        let mut bounding_box = self.settings.bounding_box();
        let delta = glam::DVec3::new(
            f64::from(node.x) - self.settings.mob_position_vec().x + bounding_box.width() / 2.0,
            f64::from(node.y) - self.settings.mob_position_vec().y + bounding_box.height() / 2.0,
            f64::from(node.z) - self.settings.mob_position_vec().z + bounding_box.depth() / 2.0,
        );
        let steps = (delta.length() / bounding_box.size()).ceil() as i32;
        if steps <= 0 {
            return true;
        }
        let step_delta = delta / f64::from(steps);

        for _ in 1..=steps {
            bounding_box = bounding_box.translate(step_delta);
            if collision.has_collision(bounding_box) {
                return false;
            }
        }

        true
    }

    fn mob_jump_height(&self) -> f64 {
        f64::from(self.settings.max_up_step()).max(1.125)
    }

    fn get_node_and_update_cost_to_max(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        path_type: PathType,
        cost: f32,
    ) -> i32 {
        let node = self.nodes.get_node(x, y, z);
        node.path_type = path_type;
        node.cost_malus = node.cost_malus.max(cost);
        node.hash()
    }

    fn get_blocked_node(&mut self, x: i32, y: i32, z: i32) -> i32 {
        let node = self.nodes.get_node(x, y, z);
        node.path_type = PathType::Blocked;
        node.cost_malus = -1.0;
        node.hash()
    }

    fn get_closed_node(&mut self, x: i32, y: i32, z: i32, path_type: PathType) -> i32 {
        let node = self.nodes.get_node(x, y, z);
        node.closed = true;
        node.path_type = path_type;
        node.cost_malus = path_type.default_malus();
        node.hash()
    }
}

const fn clockwise_direction_index(index: usize) -> usize {
    (index + 1) % VANILLA_HORIZONTAL_DIRECTIONS.len()
}

pub trait WalkNodeCollision {
    fn has_collision(&mut self, aabb: WorldAabb) -> bool;
}

impl<F> WalkNodeCollision for F
where
    F: FnMut(WorldAabb) -> bool,
{
    fn has_collision(&mut self, aabb: WorldAabb) -> bool {
        self(aabb)
    }
}

const fn does_block_have_partial_collision(path_type: PathType) -> bool {
    matches!(
        path_type,
        PathType::Fence | PathType::DoorWoodClosed | PathType::DoorIronClosed
    )
}

pub struct WalkPathEvaluator;

impl WalkPathEvaluator {
    #[must_use]
    pub fn path_type(context: &mut PathfindingContext<'_>, x: i32, y: i32, z: i32) -> PathType {
        Self::path_type_static(context, BlockPos::new(x, y, z))
    }

    #[must_use]
    pub fn path_type_static(context: &mut PathfindingContext<'_>, pos: BlockPos) -> PathType {
        let x = pos.x();
        let y = pos.y();
        let z = pos.z();
        let block_path_type = context.get_path_type_from_state(x, y, z);
        if block_path_type != PathType::Open || y < context.level().min_y() + 1 {
            return block_path_type;
        }

        match context.get_path_type_from_state(x, y - 1, z) {
            PathType::Open | PathType::Water | PathType::Lava | PathType::Walkable => {
                PathType::Open
            }
            PathType::Fire => PathType::Fire,
            PathType::Damaging => PathType::Damaging,
            PathType::StickyHoney => PathType::StickyHoney,
            PathType::PowderSnow => PathType::OnTopOfPowderSnow,
            PathType::DamageCautious => PathType::DamageCautious,
            PathType::Trapdoor => PathType::OnTopOfTrapdoor,
            _ => Self::check_neighbour_blocks(context, x, y, z, PathType::Walkable),
        }
    }

    #[must_use]
    pub fn check_neighbour_blocks(
        context: &mut PathfindingContext<'_>,
        x: i32,
        y: i32,
        z: i32,
        block_path_type: PathType,
    ) -> PathType {
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }

                    match context.get_path_type_from_state(x + dx, y + dy, z + dz) {
                        PathType::Damaging => return PathType::DamagingInNeighbor,
                        PathType::Fire | PathType::Lava => return PathType::FireInNeighbor,
                        PathType::Water => return PathType::WaterBorder,
                        PathType::DamageCautious => return PathType::DamageCautious,
                        _ => {}
                    }
                }
            }
        }

        block_path_type
    }

    #[must_use]
    pub fn path_type_from_state(level: &dyn LevelReader, pos: BlockPos) -> PathType {
        let block_state = level.get_block_state(pos);
        let block = block_state.get_block();
        if block_state.is_air() {
            return PathType::Open;
        }

        if block.has_tag(&BlockTag::TRAPDOORS)
            || block == &vanilla_blocks::LILY_PAD
            || block == &vanilla_blocks::BIG_DRIPLEAF
        {
            return PathType::Trapdoor;
        }

        if block == &vanilla_blocks::POWDER_SNOW {
            return PathType::PowderSnow;
        }

        if block == &vanilla_blocks::CACTUS || block == &vanilla_blocks::SWEET_BERRY_BUSH {
            return PathType::Damaging;
        }

        if block == &vanilla_blocks::HONEY_BLOCK {
            return PathType::StickyHoney;
        }

        if block == &vanilla_blocks::COCOA {
            return PathType::Cocoa;
        }

        if block == &vanilla_blocks::WITHER_ROSE || block.has_tag(&BlockTag::SPELEOTHEMS) {
            return PathType::DamageCautious;
        }

        let fluid_state = block_state.get_fluid_state();
        if fluid_state.is_lava() {
            return PathType::Lava;
        }

        if Self::is_burning_block(block_state) {
            return PathType::Fire;
        }

        if block.has_tag(&BlockTag::DOORS) {
            return if block_state
                .try_get_value(&BlockStateProperties::OPEN)
                .unwrap_or(false)
            {
                PathType::DoorOpen
            } else if block.has_tag(&BlockTag::MOB_INTERACTABLE_DOORS) {
                PathType::DoorWoodClosed
            } else {
                PathType::DoorIronClosed
            };
        }

        if block.has_tag(&BlockTag::RAILS) {
            return PathType::Rail;
        }

        if block.has_tag(&BlockTag::LEAVES) {
            return PathType::Leaves;
        }

        if block.has_tag(&BlockTag::FENCES)
            || block.has_tag(&BlockTag::WALLS)
            || block.has_tag(&BlockTag::FENCE_GATES)
                && !block_state
                    .try_get_value(&BlockStateProperties::OPEN)
                    .unwrap_or(false)
        {
            return PathType::Fence;
        }

        if !block_state.is_pathfindable(PathComputationType::Land) {
            return PathType::Blocked;
        }

        if fluid_state.is_water() {
            PathType::Water
        } else {
            PathType::Open
        }
    }

    #[must_use]
    pub fn is_burning_block(block_state: steel_utils::BlockStateId) -> bool {
        let block = block_state.get_block();
        block.has_tag(&BlockTag::FIRE)
            || block == &vanilla_blocks::LAVA
            || block == &vanilla_blocks::MAGMA_BLOCK
            || block == &vanilla_blocks::LAVA_CAULDRON
            || block.has_tag(&BlockTag::CAMPFIRES)
                && block_state
                    .try_get_value(&BlockStateProperties::LIT)
                    .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::blocks::block_state_ext::BlockStateExt as _;
    use steel_registry::blocks::properties::{BlockStateProperties, SlabType};
    use steel_registry::{
        REGISTRY, test_support::init_test_registry, vanilla_blocks, vanilla_entities,
    };
    use steel_utils::{BlockPos, BlockStateId, Direction, WorldAabb};

    use super::{AcceptedNodeRequest, MobPathSettings, WalkNodeEvaluator, WalkPathEvaluator};
    use crate::behavior::{BlockStateBehaviorExt as _, init_behaviors};
    use crate::entity::Mob as _;
    use crate::entity::ai::path::{
        PathComputationType, PathType, PathfindingContext, PathfindingMalus,
    };
    use crate::entity::entities::PigEntity;
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
    fn mob_path_settings_reads_can_open_doors_from_navigation() {
        init_test_registry();
        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        pig.mob_base().navigation().lock().set_can_open_doors(true);

        let settings = MobPathSettings::from_mob(&pig);

        assert!(settings.can_open_doors());
    }

    #[test]
    fn mob_path_settings_reads_can_walk_over_fences_from_navigation() {
        init_test_registry();
        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        pig.mob_base()
            .navigation()
            .lock()
            .set_can_walk_over_fences(true);

        let settings = MobPathSettings::from_mob(&pig);

        assert!(settings.can_walk_over_fences());
    }

    #[test]
    fn path_type_from_state_matches_core_vanilla_special_cases() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
        let lava = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::LAVA);
        let cactus = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::CACTUS);
        let honey = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::HONEY_BLOCK);
        let pointed_dripstone = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::POINTED_DRIPSTONE);
        let sulfur_spike = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::SULFUR_SPIKE);

        assert_eq!(classify(air), PathType::Open);
        assert_eq!(classify(water), PathType::Water);
        assert_eq!(classify(lava), PathType::Lava);
        assert_eq!(classify(cactus), PathType::Damaging);
        assert_eq!(classify(honey), PathType::StickyHoney);
        assert_eq!(classify(pointed_dripstone), PathType::DamageCautious);
        assert_eq!(classify(sulfur_spike), PathType::DamageCautious);
    }

    #[test]
    fn doors_use_vanilla_mob_interactable_door_tag() {
        init_test_registry();
        init_behaviors();

        let oak_closed = vanilla_blocks::OAK_DOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, false);
        let iron_closed = vanilla_blocks::IRON_DOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, false);
        let copper_closed = vanilla_blocks::COPPER_DOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, false);
        let oak_open = oak_closed.set_value(&BlockStateProperties::OPEN, true);

        assert_eq!(classify(oak_closed), PathType::DoorWoodClosed);
        assert_eq!(classify(copper_closed), PathType::DoorWoodClosed);
        assert_eq!(classify(iron_closed), PathType::DoorIronClosed);
        assert_eq!(classify(oak_open), PathType::DoorOpen);
    }

    #[test]
    fn block_state_pathfindable_uses_behavior_overrides() {
        init_test_registry();
        init_behaviors();

        let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
        let lava = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::LAVA);
        let cactus = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::CACTUS);
        let cocoa = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::COCOA);
        let powder_snow = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::POWDER_SNOW);
        let shallow_snow = vanilla_blocks::SNOW
            .default_state()
            .set_value(&BlockStateProperties::LAYERS, 4);
        let deep_snow = shallow_snow.set_value(&BlockStateProperties::LAYERS, 5);
        let oak_closed = vanilla_blocks::OAK_DOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, false);
        let oak_open = oak_closed.set_value(&BlockStateProperties::OPEN, true);

        assert!(water.is_pathfindable(PathComputationType::Land));
        assert!(!lava.is_pathfindable(PathComputationType::Land));
        assert!(!cactus.is_pathfindable(PathComputationType::Land));
        assert!(!cocoa.is_pathfindable(PathComputationType::Land));
        assert!(!cocoa.is_pathfindable(PathComputationType::Air));
        assert!(!cocoa.is_pathfindable(PathComputationType::Water));
        assert!(powder_snow.is_pathfindable(PathComputationType::Land));
        assert!(shallow_snow.is_pathfindable(PathComputationType::Land));
        assert!(!deep_snow.is_pathfindable(PathComputationType::Land));
        assert!(!oak_closed.is_pathfindable(PathComputationType::Land));
        assert!(oak_open.is_pathfindable(PathComputationType::Air));
        assert!(!oak_open.is_pathfindable(PathComputationType::Water));
    }

    #[test]
    fn walk_node_evaluator_applies_vanilla_door_adjustments_for_mobs() {
        init_test_registry();
        init_behaviors();

        let oak_closed = vanilla_blocks::OAK_DOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, false);
        let oak_open = oak_closed.set_value(&BlockStateProperties::OPEN, true);
        let closed_level = GridLevel::new(oak_closed);
        let open_level = GridLevel::new(oak_open);
        let mut closed_context = PathfindingContext::new(&closed_level, BlockPos::ZERO);
        let mut open_context = PathfindingContext::new(&open_level, BlockPos::ZERO);

        let opener = WalkNodeEvaluator::new(
            test_settings(1, 1, 1)
                .with_can_open_doors(true)
                .with_can_pass_doors(true),
        );
        let blocker = WalkNodeEvaluator::new(test_settings(1, 1, 1).with_can_pass_doors(false));

        assert_eq!(
            opener.get_path_type_of_mob(&mut closed_context, 0, 64, 0),
            PathType::WalkableDoor
        );
        assert_eq!(
            blocker.get_path_type_of_mob(&mut open_context, 0, 64, 0),
            PathType::Blocked
        );
    }

    #[test]
    fn walk_node_evaluator_marks_rails_unpassable_when_mob_is_not_on_rails() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let rail = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::RAIL);
        let level = GridLevel::new(air).with(BlockPos::new(1, 64, 0), rail);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));

        assert_eq!(
            evaluator.get_path_type_of_mob(&mut context, 1, 64, 0),
            PathType::UnpassableRail
        );
    }

    #[test]
    fn large_walk_node_evaluator_caps_nearby_danger_cost_like_vanilla() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
        let level = GridLevel::new(air)
            .with(BlockPos::new(0, 63, 0), stone)
            .with(BlockPos::new(3, 64, 0), water);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let evaluator = WalkNodeEvaluator::new(test_settings(4, 1, 1));

        assert_eq!(
            evaluator.get_path_type_of_mob(&mut context, 0, 64, 0),
            PathType::BigMobsCloseToDanger
        );
    }

    #[test]
    fn walk_node_evaluator_floor_level_uses_collision_shape_below_node() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let bottom_slab = vanilla_blocks::SMOOTH_STONE_SLAB
            .default_state()
            .set_value(&BlockStateProperties::SLAB_TYPE, SlabType::Bottom);
        let level = GridLevel::new(air).with(BlockPos::new(0, 63, 0), bottom_slab);
        let context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));

        assert_eq!(
            evaluator
                .get_floor_level(&context, BlockPos::new(0, 64, 0))
                .to_bits(),
            63.5_f64.to_bits()
        );
    }

    #[test]
    fn get_start_uses_grounded_mob_block_position() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let level = GridLevel::new(air).with(BlockPos::new(0, 63, 0), stone);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));

        let start = evaluator.get_start(&mut context);

        let Some(node) = evaluator.node(start) else {
            panic!("start node should exist");
        };
        assert_eq!((node.x, node.y, node.z), (0, 64, 0));
        assert_eq!(node.path_type, PathType::Walkable);
        assert_eq!(node.cost_malus.to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn get_start_floats_to_top_water_node_when_mob_can_float() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
        let level = GridLevel::new(air)
            .with(BlockPos::new(0, 64, 0), water)
            .with(BlockPos::new(0, 65, 0), water);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let mut evaluator = WalkNodeEvaluator::new(
            test_settings(1, 1, 1)
                .with_can_float(true)
                .with_in_water(true)
                .with_on_ground(false),
        );

        let start = evaluator.get_start(&mut context);

        let Some(node) = evaluator.node(start) else {
            panic!("start node should exist");
        };
        assert_eq!((node.x, node.y, node.z), (0, 65, 0));
        assert_eq!(node.path_type, PathType::Water);
    }

    #[test]
    fn get_start_scans_down_to_first_ground_when_airborne() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let level = GridLevel::new(air).with(BlockPos::new(0, 62, 0), stone);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1).with_on_ground(false));

        let start = evaluator.get_start(&mut context);

        let Some(node) = evaluator.node(start) else {
            panic!("start node should exist");
        };
        assert_eq!((node.x, node.y, node.z), (0, 63, 0));
        assert_eq!(node.path_type, PathType::Walkable);
    }

    #[test]
    fn get_start_uses_first_startable_bounding_box_corner() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let level = GridLevel::new(air).with(BlockPos::new(0, 63, 1), stone);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));

        let start = evaluator.get_start(&mut context);

        let Some(node) = evaluator.node(start) else {
            panic!("start node should exist");
        };
        assert_eq!((node.x, node.y, node.z), (0, 64, 1));
        assert_eq!(node.path_type, PathType::Walkable);
    }

    #[test]
    fn find_accepted_node_records_walkable_node_cost() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let level = GridLevel::new(air).with(BlockPos::new(0, 63, 0), stone);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));
        let mut no_collision = |_aabb: WorldAabb| false;

        let accepted = evaluator.find_accepted_node(
            &mut context,
            &mut no_collision,
            accepted_request(BlockPos::new(0, 64, 0), 0, 64.0, PathType::Walkable),
        );

        let Some(node) = accepted.and_then(|hash| evaluator.node(hash)) else {
            panic!("walkable node should be accepted");
        };
        assert_eq!(node.path_type, PathType::Walkable);
        assert_eq!(node.cost_malus.to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn find_accepted_node_falls_to_ground_when_within_max_fall_distance() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let level = GridLevel::new(air).with(BlockPos::new(0, 64, 0), stone);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 67, 0));
        let mut evaluator =
            WalkNodeEvaluator::new(test_settings(1, 1, 1).with_max_fall_distance(3));
        let mut no_collision = |_aabb: WorldAabb| false;

        let accepted = evaluator.find_accepted_node(
            &mut context,
            &mut no_collision,
            accepted_request(BlockPos::new(0, 67, 0), 0, 66.0, PathType::Open),
        );

        let Some(node) = accepted.and_then(|hash| evaluator.node(hash)) else {
            panic!("open node should fall to ground");
        };
        assert_eq!((node.x, node.y, node.z), (0, 65, 0));
        assert_eq!(node.path_type, PathType::Walkable);
    }

    #[test]
    fn find_accepted_node_blocks_falls_past_max_fall_distance() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let level = GridLevel::new(air).with(BlockPos::new(0, 64, 0), stone);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 67, 0));
        let mut evaluator =
            WalkNodeEvaluator::new(test_settings(1, 1, 1).with_max_fall_distance(1));
        let mut no_collision = |_aabb: WorldAabb| false;

        let accepted = evaluator.find_accepted_node(
            &mut context,
            &mut no_collision,
            accepted_request(BlockPos::new(0, 67, 0), 0, 66.0, PathType::Open),
        );

        let Some(node) = accepted.and_then(|hash| evaluator.node(hash)) else {
            panic!("excessive fall should produce a blocked node");
        };
        assert_eq!((node.x, node.y, node.z), (0, 65, 0));
        assert_eq!(node.path_type, PathType::Blocked);
        assert_eq!(node.cost_malus.to_bits(), (-1.0_f32).to_bits());
    }

    #[test]
    fn find_accepted_node_keeps_last_water_node_before_non_water() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let level = GridLevel::new(air)
            .with(BlockPos::new(0, 64, 0), water)
            .with(BlockPos::new(0, 63, 0), water)
            .with(BlockPos::new(0, 62, 0), stone);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));
        let mut no_collision = |_aabb: WorldAabb| false;

        let accepted = evaluator.find_accepted_node(
            &mut context,
            &mut no_collision,
            accepted_request(BlockPos::new(0, 64, 0), 0, 64.0, PathType::Water),
        );

        let Some(node) = accepted.and_then(|hash| evaluator.node(hash)) else {
            panic!("water scan should keep the deepest water node");
        };
        assert_eq!((node.x, node.y, node.z), (0, 63, 0));
        assert_eq!(node.path_type, PathType::Water);
    }

    #[test]
    fn find_accepted_node_rejects_partial_collision_when_reach_is_blocked() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let level = GridLevel::new(air).with(BlockPos::new(0, 63, 0), stone);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));
        let mut collision_checked = false;
        let mut blocked = |_aabb: WorldAabb| {
            collision_checked = true;
            true
        };

        let accepted = evaluator.find_accepted_node(
            &mut context,
            &mut blocked,
            accepted_request(BlockPos::new(0, 64, 0), 0, 64.0, PathType::DoorWoodClosed),
        );

        assert!(accepted.is_none());
        assert!(collision_checked);
    }

    #[test]
    fn get_neighbors_expands_all_cardinal_and_diagonal_nodes_on_flat_ground() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let mut level = GridLevel::new(air);
        for x in -1..=1 {
            for z in -1..=1 {
                level = level.with(BlockPos::new(x, 63, z), stone);
            }
        }
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let mut evaluator = WalkNodeEvaluator::new(test_settings(1, 1, 1));
        let mut no_collision = |_aabb: WorldAabb| false;
        let Some(current) = evaluator.find_accepted_node(
            &mut context,
            &mut no_collision,
            accepted_request(BlockPos::new(0, 64, 0), 0, 64.0, PathType::Walkable),
        ) else {
            panic!("current walkable node should be accepted");
        };

        let neighbors = evaluator.get_neighbors(&mut context, &mut no_collision, current);

        assert_eq!(neighbors.len(), 8);
        let positions = neighbor_positions(&evaluator, &neighbors);
        assert!(positions.contains(&(0, 64, -1)));
        assert!(positions.contains(&(1, 64, 0)));
        assert!(positions.contains(&(0, 64, 1)));
        assert!(positions.contains(&(-1, 64, 0)));
        assert!(positions.contains(&(1, 64, -1)));
        assert!(positions.contains(&(1, 64, 1)));
        assert!(positions.contains(&(-1, 64, 1)));
        assert!(positions.contains(&(-1, 64, -1)));
    }

    #[test]
    fn get_neighbors_rejects_diagonals_through_walkable_doors() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let oak_closed = vanilla_blocks::OAK_DOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, false);
        let mut level = GridLevel::new(air).with(BlockPos::new(0, 64, -1), oak_closed);
        for x in -1..=1 {
            for z in -1..=1 {
                level = level.with(BlockPos::new(x, 63, z), stone);
            }
        }
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));
        let mut evaluator = WalkNodeEvaluator::new(
            test_settings(1, 1, 1)
                .with_can_open_doors(true)
                .with_can_pass_doors(true),
        );
        let mut no_collision = |_aabb: WorldAabb| false;
        let Some(current) = evaluator.find_accepted_node(
            &mut context,
            &mut no_collision,
            accepted_request(BlockPos::new(0, 64, 0), 0, 64.0, PathType::Walkable),
        ) else {
            panic!("current walkable node should be accepted");
        };

        let neighbors = evaluator.get_neighbors(&mut context, &mut no_collision, current);

        let positions = neighbor_positions(&evaluator, &neighbors);
        assert!(positions.contains(&(0, 64, -1)));
        assert!(positions.contains(&(1, 64, 0)));
        assert!(positions.contains(&(-1, 64, 0)));
        assert!(!positions.contains(&(1, 64, -1)));
        assert!(!positions.contains(&(-1, 64, -1)));
    }

    #[test]
    fn open_air_above_solid_ground_becomes_walkable() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let level = GridLevel::new(air).with(BlockPos::new(0, 63, 0), stone);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));

        assert_eq!(
            WalkPathEvaluator::path_type_static(&mut context, BlockPos::new(0, 64, 0)),
            PathType::Walkable
        );
    }

    #[test]
    fn walkable_ground_adjacent_to_water_becomes_water_border() {
        init_test_registry();
        init_behaviors();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
        let level = GridLevel::new(air)
            .with(BlockPos::new(0, 63, 0), stone)
            .with(BlockPos::new(1, 64, 0), water);
        let mut context = PathfindingContext::new(&level, BlockPos::new(0, 64, 0));

        assert_eq!(
            WalkPathEvaluator::path_type_static(&mut context, BlockPos::new(0, 64, 0)),
            PathType::WaterBorder
        );
    }

    fn classify(state: BlockStateId) -> PathType {
        let level = GridLevel::new(state);
        WalkPathEvaluator::path_type_from_state(&level, BlockPos::ZERO)
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

    const fn accepted_request(
        pos: BlockPos,
        jump_size: i32,
        node_height: f64,
        current_path_type: PathType,
    ) -> AcceptedNodeRequest {
        AcceptedNodeRequest {
            pos,
            jump_size,
            node_height,
            travel_direction: Direction::North,
            current_path_type,
        }
    }

    fn neighbor_positions(
        evaluator: &WalkNodeEvaluator,
        neighbors: &super::WalkNeighbors,
    ) -> Vec<(i32, i32, i32)> {
        neighbors
            .iter()
            .filter_map(|hash| evaluator.node(hash).map(|node| (node.x, node.y, node.z)))
            .collect()
    }
}
