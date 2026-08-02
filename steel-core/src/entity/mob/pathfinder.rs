use std::sync::Arc;

use glam::DVec3;
use steel_math::fast_floor;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{vanilla_attributes, vanilla_blocks};
use steel_utils::{BlockPos, ChunkPos};

use super::{Mob, TARGET_REACH_DISTANCE_SQR};
use crate::entity::ai::navigation::{
    NavigationPathRequest, NavigationRecomputeRequest, NavigationTickContext,
};
use crate::entity::ai::path::Path;
use crate::entity::ai::walk::{MobPathSettings, WalkNodeEvaluator};
use crate::entity::{Entity, LivingEntity, SharedEntity};
use crate::physics::WorldCollisionProvider;
use crate::world::{LevelReader, World};

pub(super) fn tick_path_navigation_target<M: Mob + ?Sized>(
    mob: &M,
    world: &Arc<World>,
    game_time: i64,
    can_update_path: bool,
) {
    let (target, speed_modifier) = {
        let mut navigation = mob.mob_base().navigation().lock();
        let mob_position =
            ground_navigation_temp_mob_pos(mob, world.as_ref(), navigation.can_float());
        let context = NavigationTickContext {
            mob_position,
            mob_bounding_box_width: mob.bounding_box().width(),
            mob_speed: mob.get_speed(),
            game_time,
        };
        let next_target = if can_update_path {
            navigation.next_move_target(context)
        } else {
            navigation.next_move_target_without_path_update(context, mob.on_ground())
        };
        let Some(target) = next_target else {
            return;
        };
        target
    };

    let target_pos = BlockPos::containing(target.x, target.y, target.z);
    let ground_y = if world.get_block_state(target_pos.below()).is_air() {
        target.y
    } else {
        WalkNodeEvaluator::floor_level(world.as_ref(), target_pos)
    };
    mob.set_wanted_position(DVec3::new(target.x, ground_y, target.z), speed_modifier);
}

fn ground_navigation_temp_mob_pos<M: Mob + ?Sized>(
    mob: &M,
    world: &World,
    can_float: bool,
) -> DVec3 {
    let position = mob.position();
    DVec3::new(
        position.x,
        f64::from(ground_navigation_surface_y(mob, world, can_float)),
        position.z,
    )
}

fn ground_navigation_surface_y<M: Mob + ?Sized>(mob: &M, world: &World, can_float: bool) -> i32 {
    if !mob.is_in_water() || !can_float {
        return fast_floor(mob.position().y + 0.5);
    }

    let position = mob.position();
    let block_y = mob.block_position().y();
    let mut surface = block_y;
    let mut state = world.get_block_state(BlockPos::containing(
        position.x,
        f64::from(surface),
        position.z,
    ));
    let mut steps = 0;
    while state.get_block() == &vanilla_blocks::WATER {
        surface += 1;
        state = world.get_block_state(BlockPos::containing(
            position.x,
            f64::from(surface),
            position.z,
        ));
        steps += 1;
        if steps > 16 {
            return block_y;
        }
    }

    surface
}

pub trait PathfinderMob: Mob {
    fn controlled_pathfinder_vehicle(&self) -> Option<SharedEntity> {
        let vehicle = self.controlled_mob_vehicle()?;
        vehicle.as_pathfinder_mob()?;
        Some(vehicle)
    }

    fn get_walk_target_value(&self, pos: BlockPos) -> f32 {
        self.as_animal()
            .map_or(0.0, |animal| animal.animal_walk_target_value(pos))
    }

    fn has_line_of_sight_cached(&self, target: &dyn Entity) -> bool {
        self.mob_base()
            .sensing()
            .lock()
            .has_line_of_sight(target.id(), || self.has_line_of_sight(target))
    }

    fn can_update_path(&self) -> bool {
        self.on_ground() || self.is_in_water() || self.is_in_lava() || self.is_passenger()
    }

    fn can_path_to_targets_below_surface(&self) -> bool {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            return pathfinder.can_path_to_targets_below_surface();
        }

        self.mob_base()
            .navigation()
            .lock()
            .can_path_to_targets_below_surface()
    }

    fn can_reach_living_target(&self, target: &dyn LivingEntity) -> bool {
        let target_pos = target.block_position();
        self.create_path_to(target_pos, 0)
            .is_some_and(|path| path_end_node_can_reach_target(&path, target_pos))
    }

    fn tick_pathfinder_path_navigation(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let game_time = world.game_time();
        let recompute_request = {
            let mut navigation = self.mob_base().navigation().lock();
            navigation.tick();
            navigation.take_delayed_recompute_request(game_time, self.can_update_path())
        };
        if let Some(request) = recompute_request {
            self.recompute_path(request);
        }

        tick_path_navigation_target(self, &world, game_time, self.can_update_path());
    }

    fn tick_pathfinder_goal_selectors(&self)
    where
        Self: Sized,
    {
        let id_based_tick_count = self.tick_count().wrapping_add(self.id());
        let mut target_selector = self.mob_base().target_selector().lock();
        let mut goal_selector = self.mob_base().goal_selector().lock();
        if id_based_tick_count % 2 != 0 && self.tick_count() > 1 {
            target_selector.tick_running_goals(self, false);
            goal_selector.tick_running_goals(self, false);
        } else {
            target_selector.tick(self);
            goal_selector.tick(self);
        }
    }

    fn is_stable_destination(&self, pos: BlockPos) -> bool {
        self.level()
            .is_some_and(|world| world.get_block_state(pos.below()).is_solid_render())
    }

    fn create_path_to(&self, target: BlockPos, reach_range: i32) -> Option<Path> {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            return pathfinder.create_path_to(target, reach_range);
        }

        let world = self.level()?;
        if !world.has_full_chunk(ChunkPos::from_block_pos(target)) {
            return None;
        }

        let target = path_target_for_mob(self, world.as_ref(), target);
        let targets = [target];
        self.create_path_to_targets(&world, &targets, reach_range)
    }

    fn recompute_path(&self, request: NavigationRecomputeRequest) {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            pathfinder.recompute_path(request);
            return;
        }

        let path = self.create_path_to(request.target_pos, request.reach_range);
        self.mob_base()
            .navigation()
            .lock()
            .complete_recompute_path(path, request.game_time);
    }

    fn move_to_pos(&self, target: DVec3, speed_modifier: f64) -> bool {
        self.move_to_pos_with_reach(target, 1, speed_modifier)
    }

    fn move_to_pos_with_reach(&self, target: DVec3, reach_range: i32, speed_modifier: f64) -> bool {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            return pathfinder.move_to_pos_with_reach(target, reach_range, speed_modifier);
        }

        let target_pos = BlockPos::containing(target.x, target.y, target.z);
        let Some(world) = self.level() else {
            self.mob_base().navigation().lock().stop();
            return false;
        };
        if !world.has_full_chunk(ChunkPos::from_block_pos(target_pos)) {
            self.mob_base().navigation().lock().stop();
            return false;
        }

        let target_pos = path_target_for_mob(self, world.as_ref(), target_pos);
        let targets = [target_pos];
        if self
            .mob_base()
            .navigation()
            .lock()
            .reuse_current_path_to_targets(
                world.as_ref(),
                &targets,
                speed_modifier,
                self.position(),
            )
        {
            return true;
        }

        let path = self.create_path_to_targets(&world, &targets, reach_range);
        self.move_to_path(path, speed_modifier)
    }

    fn move_to_path(&self, path: Option<Path>, speed_modifier: f64) -> bool {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            return pathfinder.move_to_path(path, speed_modifier);
        }

        let Some(world) = self.level() else {
            self.mob_base().navigation().lock().stop();
            return false;
        };
        let mut navigation = self.mob_base().navigation().lock();
        let Some(path) = path else {
            navigation.stop();
            return false;
        };

        navigation.move_to(world.as_ref(), path, speed_modifier, self.position())
    }

    fn is_path_finding(&self) -> bool {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            return pathfinder.is_path_finding();
        }

        !self.mob_base().navigation().lock().is_done()
    }

    fn is_panicking(&self) -> bool {
        self.mob_base()
            .goal_selector()
            .lock()
            .has_running_panic_goal()
    }

    fn create_path_to_targets(
        &self,
        world: &Arc<World>,
        targets: &[BlockPos],
        reach_range: i32,
    ) -> Option<Path> {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            return pathfinder.create_path_to_targets(world, targets, reach_range);
        }

        if targets.is_empty()
            || self.position().y < f64::from(world.min_y())
            || !self.can_update_path()
        {
            return None;
        }

        let follow_range = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::FOLLOW_RANGE);
        let max_path_length = {
            let mut navigation = self.mob_base().navigation().lock();
            navigation.update_pathfinder_max_visited_nodes(follow_range);
            navigation.max_path_length(follow_range)
        };

        let mob_position = self.block_position();
        let settings = MobPathSettings::from_mob(self);
        let mut evaluator = WalkNodeEvaluator::new(settings);
        let collision_world =
            WorldCollisionProvider::for_path_navigation(world, self.as_entity_event_source());
        let mut collision = |aabb| {
            collision_world.has_entity_context_collision(
                aabb,
                self.position().y,
                self.is_descending(),
            )
        };

        self.mob_base().navigation().lock().create_path(
            &mut evaluator,
            world.as_ref(),
            &mut collision,
            NavigationPathRequest {
                mob_position,
                targets,
                max_path_length,
                reach_range,
            },
        )
    }
}

pub(super) fn path_end_node_can_reach_target(path: &Path, target: BlockPos) -> bool {
    let Some(end_node) = path.end_node() else {
        return false;
    };
    let dx = end_node.x - target.x();
    let dz = end_node.z - target.z();
    f64::from(dx * dx + dz * dz) <= TARGET_REACH_DISTANCE_SQR
}

fn path_target_for_mob<M: PathfinderMob + ?Sized>(
    mob: &M,
    level: &dyn LevelReader,
    target: BlockPos,
) -> BlockPos {
    if mob.can_path_to_targets_below_surface() {
        target
    } else {
        find_ground_path_target_surface(level, target)
    }
}

pub(super) fn find_ground_path_target_surface(
    level: &dyn LevelReader,
    mut pos: BlockPos,
) -> BlockPos {
    if level.get_block_state(pos).is_air() {
        let mut column_pos = pos.below();
        while column_pos.y() >= level.min_y() && level.get_block_state(column_pos).is_air() {
            column_pos = column_pos.below();
        }
        if column_pos.y() >= level.min_y() {
            return column_pos.above();
        }

        column_pos = pos.at_y(pos.y() + 1);
        while column_pos.y() < level.max_y_exclusive() && level.get_block_state(column_pos).is_air()
        {
            column_pos = column_pos.above();
        }
        pos = column_pos;
    }

    if !level.get_block_state(pos).is_solid() {
        return pos;
    }

    let mut column_pos = pos.above();
    while column_pos.y() < level.max_y_exclusive() && level.get_block_state(column_pos).is_solid() {
        column_pos = column_pos.above();
    }
    column_pos
}
