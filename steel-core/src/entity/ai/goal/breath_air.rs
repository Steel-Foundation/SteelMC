use glam::DVec3;
use steel_math::fast_floor;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::vanilla_blocks;
use steel_utils::BlockPos;

use super::selector::{Goal, GoalControls};
use crate::behavior::BlockStateBehaviorExt as _;
use crate::entity::PathfinderMob;
use crate::entity::ai::path::PathComputationType;
use crate::physics::MoverType;
use crate::world::LevelReader;

const BREATH_AIR_THRESHOLD: i32 = 140;
const AIR_SEARCH_HORIZONTAL_RADIUS: f64 = 1.0;
const AIR_SEARCH_VERTICAL_ABOVE: f64 = 8.0;
const BREATH_MOVE_SPEED: f32 = 0.02;
const AIR_NAVIGATION_SPEED_MODIFIER: f64 = 1.0;

pub struct BreathAirGoal;

impl BreathAirGoal {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Goal for BreathAirGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        mob.air_supply() < BREATH_AIR_THRESHOLD
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.can_use(mob)
    }

    fn is_interruptable(&self) -> bool {
        false
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        find_air_position(mob);
        mob.mob_base().navigation().lock().stop();
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        find_air_position(mob);
        let input = mob.travel_input();
        mob.move_relative(
            BREATH_MOVE_SPEED,
            DVec3::new(
                f64::from(input.sideways()),
                f64::from(input.vertical()),
                f64::from(input.forward()),
            ),
        );
        mob.move_entity(MoverType::SelfMovement, mob.velocity());
    }
}

fn find_air_position(mob: &dyn PathfinderMob) {
    let position = mob.position();
    let destination_pos = mob
        .level()
        .and_then(|world| first_air_position(world.as_ref(), position))
        .unwrap_or_else(|| BlockPos::containing(position.x, position.y + 8.0, position.z));

    mob.move_to_pos(
        DVec3::new(
            f64::from(destination_pos.x()),
            f64::from(destination_pos.y() + 1),
            f64::from(destination_pos.z()),
        ),
        AIR_NAVIGATION_SPEED_MODIFIER,
    );
}

fn first_air_position(level: &dyn LevelReader, position: DVec3) -> Option<BlockPos> {
    let (min, max) = air_search_bounds(position);
    first_matching_pos_in_closed_box(min, max, |pos| gives_air(level, pos))
}

fn air_search_bounds(position: DVec3) -> (BlockPos, BlockPos) {
    (
        BlockPos::new(
            fast_floor(position.x - AIR_SEARCH_HORIZONTAL_RADIUS),
            fast_floor(position.y),
            fast_floor(position.z - AIR_SEARCH_HORIZONTAL_RADIUS),
        ),
        BlockPos::new(
            fast_floor(position.x + AIR_SEARCH_HORIZONTAL_RADIUS),
            fast_floor(position.y + AIR_SEARCH_VERTICAL_ABOVE),
            fast_floor(position.z + AIR_SEARCH_HORIZONTAL_RADIUS),
        ),
    )
}

fn first_matching_pos_in_closed_box(
    min: BlockPos,
    max: BlockPos,
    mut predicate: impl FnMut(BlockPos) -> bool,
) -> Option<BlockPos> {
    for z in min.z()..=max.z() {
        for y in min.y()..=max.y() {
            for x in min.x()..=max.x() {
                let pos = BlockPos::new(x, y, z);
                if predicate(pos) {
                    return Some(pos);
                }
            }
        }
    }
    None
}

fn gives_air(level: &dyn LevelReader, pos: BlockPos) -> bool {
    let state = level.get_block_state(pos);
    (!state.has_fluid() || state.get_block() == &vanilla_blocks::BUBBLE_COLUMN)
        && state.is_pathfindable(PathComputationType::Land)
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::entities::PigEntity;
    use crate::entity::{Entity, LivingEntity, LivingTravelInput};

    #[test]
    fn breath_air_goal_uses_move_and_look_controls() {
        let goal = BreathAirGoal::new();

        assert_eq!(goal.controls(), GoalControls::MOVE | GoalControls::LOOK);
        assert!(!goal.is_interruptable());
    }

    #[test]
    fn breath_air_goal_uses_vanilla_air_threshold() {
        init_test_registry();
        let mut goal = BreathAirGoal::new();
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        mob.set_air_supply(BREATH_AIR_THRESHOLD);
        assert!(!goal.can_use(&mob));

        mob.set_air_supply(BREATH_AIR_THRESHOLD - 1);
        assert!(goal.can_use(&mob));
        assert!(goal.can_continue_to_use(&mob));
    }

    #[test]
    fn breath_air_goal_tick_applies_travel_input_to_velocity() {
        init_test_registry();
        let mut goal = BreathAirGoal::new();
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        mob.set_travel_input(LivingTravelInput::new(1.0, 0.0, 0.0));

        goal.tick(&mob);

        assert!(mob.velocity().length_squared() > 0.0);
    }

    #[test]
    fn air_search_bounds_match_vanilla_offsets() {
        let (min, max) = air_search_bounds(DVec3::new(-0.25, 64.9, 0.25));

        assert_eq!(min, BlockPos::new(-2, 64, -1));
        assert_eq!(max, BlockPos::new(0, 72, 1));
    }

    #[test]
    fn closed_box_scan_uses_vanilla_x_then_y_then_z_order() {
        let min = BlockPos::new(10, 20, 30);
        let max = BlockPos::new(12, 22, 32);
        let expected = BlockPos::new(11, 20, 30);
        let later_y = BlockPos::new(10, 21, 30);
        let later_z = BlockPos::new(10, 20, 31);

        let found = first_matching_pos_in_closed_box(min, max, |pos| {
            pos == expected || pos == later_y || pos == later_z
        });

        assert_eq!(found, Some(expected));
    }
}
