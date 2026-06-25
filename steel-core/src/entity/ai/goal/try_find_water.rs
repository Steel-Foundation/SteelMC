use glam::DVec3;
use steel_math::fast_floor;
use steel_utils::BlockPos;

use super::selector::{Goal, GoalControls};
use crate::behavior::BlockStateBehaviorExt as _;
use crate::entity::PathfinderMob;
use crate::fluid::FluidStateExt as _;

const WATER_SEARCH_HORIZONTAL_RADIUS: f64 = 2.0;
const WATER_SEARCH_VERTICAL_BELOW: f64 = 2.0;
const WATER_MOVE_SPEED_MODIFIER: f64 = 1.0;

pub struct TryFindWaterGoal;

impl TryFindWaterGoal {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Goal for TryFindWaterGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if !mob.on_ground() {
            return false;
        }

        let Some(world) = mob.level() else {
            return false;
        };
        !world
            .get_block_state(mob.block_position())
            .get_fluid_state()
            .is_water()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        let Some(world) = mob.level() else {
            return;
        };

        let (min, max) = water_search_bounds(mob.position());
        let Some(water_pos) = first_matching_pos_in_closed_box(min, max, |pos| {
            world.get_block_state(pos).get_fluid_state().is_water()
        }) else {
            return;
        };

        mob.mob_base()
            .controls()
            .lock()
            .move_control
            .set_wanted_position(
                DVec3::new(
                    f64::from(water_pos.x()),
                    f64::from(water_pos.y()),
                    f64::from(water_pos.z()),
                ),
                WATER_MOVE_SPEED_MODIFIER,
            );
    }
}

fn water_search_bounds(position: DVec3) -> (BlockPos, BlockPos) {
    (
        BlockPos::new(
            fast_floor(position.x - WATER_SEARCH_HORIZONTAL_RADIUS),
            fast_floor(position.y - WATER_SEARCH_VERTICAL_BELOW),
            fast_floor(position.z - WATER_SEARCH_HORIZONTAL_RADIUS),
        ),
        BlockPos::new(
            fast_floor(position.x + WATER_SEARCH_HORIZONTAL_RADIUS),
            fast_floor(position.y),
            fast_floor(position.z + WATER_SEARCH_HORIZONTAL_RADIUS),
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

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::Entity;
    use crate::entity::entities::PigEntity;

    #[test]
    fn try_find_water_goal_claims_no_controls_like_vanilla() {
        let goal = TryFindWaterGoal::new();

        assert_eq!(goal.controls(), GoalControls::EMPTY);
    }

    #[test]
    fn try_find_water_goal_requires_on_ground() {
        init_test_registry();
        let mut goal = TryFindWaterGoal::new();
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(!goal.can_use(&mob));
    }

    #[test]
    fn try_find_water_goal_requires_world_after_on_ground_check() {
        init_test_registry();
        let mut goal = TryFindWaterGoal::new();
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        mob.set_on_ground(true);

        assert!(!goal.can_use(&mob));
    }

    #[test]
    fn water_search_bounds_match_vanilla_offsets() {
        let (min, max) = water_search_bounds(DVec3::new(-0.25, 64.9, 0.25));

        assert_eq!(min, BlockPos::new(-3, 62, -2));
        assert_eq!(max, BlockPos::new(1, 64, 2));
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
