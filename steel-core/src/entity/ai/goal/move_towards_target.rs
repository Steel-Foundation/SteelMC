use std::f64::consts::FRAC_PI_2;

use glam::DVec3;

use super::random_pos::default_random_pos_towards;
use super::selector::{Goal, GoalControls};
use crate::entity::{PathfinderMob, SharedEntity};

pub struct MoveTowardsTargetGoal {
    target: Option<SharedEntity>,
    wanted_position: Option<DVec3>,
    speed_modifier: f64,
    within: f32,
}

impl MoveTowardsTargetGoal {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f64, within: f32) -> Self {
        Self {
            target: None,
            wanted_position: None,
            speed_modifier,
            within,
        }
    }

    fn within_distance_sqr(&self) -> f64 {
        f64::from(self.within * self.within)
    }
}

impl Goal for MoveTowardsTargetGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(target) = mob.target() else {
            return false;
        };

        if target.position().distance_squared(mob.position()) > self.within_distance_sqr() {
            return false;
        }

        let Some(position) = default_random_pos_towards(mob, 16, 7, target.position(), FRAC_PI_2)
        else {
            return false;
        };

        self.target = Some(target);
        self.wanted_position = Some(position);
        true
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(target) = &self.target else {
            return false;
        };

        !mob.mob_base().navigation().lock().is_done()
            && target.is_alive()
            && target.position().distance_squared(mob.position()) < self.within_distance_sqr()
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.target = None;
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if let Some(wanted_position) = self.wanted_position {
            mob.move_to_pos(wanted_position, self.speed_modifier);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::{Mob, entities::PigEntity};

    #[test]
    fn move_towards_target_goal_uses_move_control() {
        let goal = MoveTowardsTargetGoal::new(1.0, 16.0);

        assert_eq!(goal.controls(), GoalControls::MOVE);
    }

    #[test]
    fn move_towards_target_goal_requires_target() {
        init_test_registry();
        let mut goal = MoveTowardsTargetGoal::new(1.0, 16.0);
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(!goal.can_use(&mob));
    }

    #[test]
    fn move_towards_target_goal_rejects_target_outside_range() {
        init_test_registry();
        let mut goal = MoveTowardsTargetGoal::new(1.0, 8.0);
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let target: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::new(9.0, 0.0, 0.0),
            Weak::new(),
        ));
        assert!(mob.set_target(Some(&target)));

        assert!(!goal.can_use(&mob));
    }
}
