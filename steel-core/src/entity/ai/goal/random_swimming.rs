use glam::DVec3;
use steel_utils::BlockPos;

use super::random_pos::default_random_pos;
use super::random_stroll::RandomStrollGoal;
use super::selector::{Goal, GoalControls};
use crate::behavior::BlockStateBehaviorExt as _;
use crate::entity::PathfinderMob;
use crate::entity::ai::path::PathComputationType;

const RANDOM_SWIMMING_HORIZONTAL_RANGE: i32 = 10;
const RANDOM_SWIMMING_VERTICAL_RANGE: i32 = 7;
const RANDOM_SWIMMING_RETRIES: i32 = 10;

pub struct RandomSwimmingGoal {
    stroll: RandomStrollGoal,
}

impl RandomSwimmingGoal {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f64, interval: i32) -> Self {
        Self {
            stroll: RandomStrollGoal::with_interval(speed_modifier, interval),
        }
    }
}

impl Goal for RandomSwimmingGoal {
    fn controls(&self) -> GoalControls {
        self.stroll.controls()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.stroll.can_use_with_position(mob, random_swimmable_pos)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.stroll.can_continue_to_use(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.stroll.start(mob);
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.stroll.stop(mob);
    }
}

fn random_swimmable_pos(mob: &dyn PathfinderMob) -> Option<DVec3> {
    retry_random_swimmable_pos(
        default_random_pos(
            mob,
            RANDOM_SWIMMING_HORIZONTAL_RANGE,
            RANDOM_SWIMMING_VERTICAL_RANGE,
        ),
        || {
            default_random_pos(
                mob,
                RANDOM_SWIMMING_HORIZONTAL_RANGE,
                RANDOM_SWIMMING_VERTICAL_RANGE,
            )
        },
        |pos| is_water_pathfindable(mob, pos),
    )
}

fn retry_random_swimmable_pos(
    mut target_pos: Option<DVec3>,
    mut next_pos: impl FnMut() -> Option<DVec3>,
    mut is_water_pathfindable: impl FnMut(DVec3) -> bool,
) -> Option<DVec3> {
    let mut count = 0;
    loop {
        let pos = target_pos?;
        if is_water_pathfindable(pos) || count >= RANDOM_SWIMMING_RETRIES {
            return Some(pos);
        }

        count += 1;
        target_pos = next_pos();
    }
}

fn is_water_pathfindable(mob: &dyn PathfinderMob, pos: DVec3) -> bool {
    let Some(world) = mob.level() else {
        return false;
    };
    world
        .get_block_state(BlockPos::containing(pos.x, pos.y, pos.z))
        .is_pathfindable(PathComputationType::Water)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_swimming_goal_uses_random_stroll_controls() {
        let goal = RandomSwimmingGoal::new(1.0, 120);

        assert_eq!(goal.controls(), GoalControls::MOVE);
    }

    #[test]
    fn random_swimmable_retry_accepts_first_pathfindable_pos() {
        let result = retry_random_swimmable_pos(Some(DVec3::new(1.0, 2.0, 3.0)), || None, |_| true);

        assert_eq!(result, Some(DVec3::new(1.0, 2.0, 3.0)));
    }

    #[test]
    fn random_swimmable_retry_uses_next_candidate_until_pathfindable() {
        let mut next_x = 1.0;
        let result = retry_random_swimmable_pos(
            Some(DVec3::ZERO),
            || {
                let pos = DVec3::new(next_x, 0.0, 0.0);
                next_x += 1.0;
                Some(pos)
            },
            |pos| pos.x.to_bits() == 3.0_f64.to_bits(),
        );

        assert_eq!(result, Some(DVec3::new(3.0, 0.0, 0.0)));
    }

    #[test]
    fn random_swimmable_retry_returns_last_candidate_after_vanilla_retry_limit() {
        let mut next_x = 1.0;
        let result = retry_random_swimmable_pos(
            Some(DVec3::ZERO),
            || {
                let pos = DVec3::new(next_x, 0.0, 0.0);
                next_x += 1.0;
                Some(pos)
            },
            |_| false,
        );

        assert_eq!(result, Some(DVec3::new(10.0, 0.0, 0.0)));
    }
}
