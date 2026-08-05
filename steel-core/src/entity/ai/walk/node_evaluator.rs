use super::*;

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
        if self.settings.entity_width() > 1 {
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

        for dx in 0..self.settings.entity_width() {
            for dy in 0..self.settings.entity_height() {
                for dz in 0..self.settings.entity_depth() {
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
            && self.settings.can_open_doors()
            && self.settings.can_pass_doors()
        {
            return PathType::WalkableDoor;
        }

        if block_type == PathType::DoorOpen && !self.settings.can_pass_doors() {
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
