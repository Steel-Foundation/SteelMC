use std::f64::consts::FRAC_PI_2;

use glam::DVec3;

use super::random_pos::default_random_pos_towards;
use super::selector::{Goal, GoalControls};
use crate::entity::PathfinderMob;

pub struct MoveTowardsRestrictionGoal {
    wanted_position: Option<DVec3>,
    speed_modifier: f64,
}

impl MoveTowardsRestrictionGoal {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f64) -> Self {
        Self {
            wanted_position: None,
            speed_modifier,
        }
    }
}

impl Goal for MoveTowardsRestrictionGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if mob.is_within_home() {
            return false;
        }

        let (x, y, z) = mob.home_position().get_bottom_center();
        let Some(position) = default_random_pos_towards(mob, 16, 7, DVec3::new(x, y, z), FRAC_PI_2)
        else {
            return false;
        };

        self.wanted_position = Some(position);
        true
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        !mob.mob_base().navigation().lock().is_done()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if let Some(wanted_position) = self.wanted_position {
            mob.move_to_pos(wanted_position, self.speed_modifier);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use steel_registry::{test_support::init_test_registry, vanilla_entities};
    use steel_utils::BlockPos;

    use super::*;
    use crate::entity::{Mob, entities::PigEntity};

    #[test]
    fn move_towards_restriction_goal_uses_move_control() {
        let goal = MoveTowardsRestrictionGoal::new(1.0);

        assert_eq!(goal.controls(), GoalControls::MOVE);
    }

    #[test]
    fn move_towards_restriction_goal_requires_outside_home() {
        init_test_registry();
        let mut goal = MoveTowardsRestrictionGoal::new(1.0);
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        mob.set_home_to(BlockPos::ZERO, 4);

        assert!(!goal.can_use(&mob));
    }
}
