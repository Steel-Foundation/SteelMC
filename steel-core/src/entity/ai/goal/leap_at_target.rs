use glam::DVec3;

use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use crate::entity::{PathfinderMob, SharedEntity};

const MIN_LEAP_DISTANCE_SQR: f64 = 4.0;
const MAX_LEAP_DISTANCE_SQR: f64 = 16.0;
const HORIZONTAL_TARGET_EPSILON_SQR: f64 = 1.0e-7;
const HORIZONTAL_LEAP_SCALE: f64 = 0.4;
const EXISTING_MOMENTUM_SCALE: f64 = 0.2;
const LEAP_CHANCE_TICKS: i32 = 5;

pub struct LeapAtTargetGoal {
    target: Option<SharedEntity>,
    yd: f32,
}

impl LeapAtTargetGoal {
    #[must_use]
    pub(crate) const fn new(yd: f32) -> Self {
        Self { target: None, yd }
    }
}

impl Goal for LeapAtTargetGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::JUMP | GoalControls::MOVE
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if mob.has_controlling_passenger() {
            return false;
        }

        let Some(target) = mob.target() else {
            return false;
        };

        let distance_sqr = mob.position().distance_squared(target.position());
        if !(MIN_LEAP_DISTANCE_SQR..=MAX_LEAP_DISTANCE_SQR).contains(&distance_sqr) {
            return false;
        }

        if !mob.on_ground() {
            return false;
        }

        if rand::random_range(0..reduced_tick_delay(LEAP_CHANCE_TICKS)) != 0 {
            return false;
        }

        self.target = Some(target);
        true
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        !mob.on_ground()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        let Some(target) = &self.target else {
            return;
        };

        let movement = mob.velocity();
        let mut delta = target.position() - mob.position();
        delta.y = 0.0;
        if delta.length_squared() > HORIZONTAL_TARGET_EPSILON_SQR {
            delta = delta.normalize() * HORIZONTAL_LEAP_SCALE + movement * EXISTING_MOMENTUM_SCALE;
        }

        mob.set_velocity(DVec3::new(delta.x, f64::from(self.yd), delta.z));
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.target = None;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::{Entity, Mob, entities::PigEntity};

    fn pig(id: i32, position: DVec3) -> PigEntity {
        PigEntity::new(&vanilla_entities::PIG, id, position, Weak::new())
    }

    fn shared_pig(id: i32, position: DVec3) -> SharedEntity {
        Arc::new(pig(id, position))
    }

    fn set_target(mob: &PigEntity, target: &SharedEntity) {
        assert!(mob.set_target(Some(target)));
    }

    fn assert_vec3_close(left: DVec3, right: DVec3) {
        assert!(
            (left - right).abs().cmple(DVec3::splat(1.0e-12)).all(),
            "expected {left:?} to be close to {right:?}"
        );
    }

    #[test]
    fn leap_at_target_goal_uses_jump_and_move_controls() {
        let goal = LeapAtTargetGoal::new(0.4);

        assert_eq!(goal.controls(), GoalControls::JUMP | GoalControls::MOVE);
    }

    #[test]
    fn leap_at_target_goal_requires_target() {
        init_test_registry();
        let mut goal = LeapAtTargetGoal::new(0.4);
        let mob = pig(1, DVec3::ZERO);
        mob.base().set_on_ground(true);

        assert!(!goal.can_use(&mob));
    }

    #[test]
    fn leap_at_target_goal_uses_vanilla_distance_window() {
        init_test_registry();
        let mut goal = LeapAtTargetGoal::new(0.4);
        let mob = pig(1, DVec3::ZERO);
        mob.base().set_on_ground(true);

        let close_target = shared_pig(2, DVec3::new(1.0, 0.0, 0.0));
        set_target(&mob, &close_target);
        assert!(!goal.can_use(&mob));

        let far_target = shared_pig(3, DVec3::new(5.0, 0.0, 0.0));
        set_target(&mob, &far_target);
        assert!(!goal.can_use(&mob));
    }

    #[test]
    fn leap_at_target_goal_requires_ground() {
        init_test_registry();
        let mut goal = LeapAtTargetGoal::new(0.4);
        let mob = pig(1, DVec3::ZERO);
        let target = shared_pig(2, DVec3::new(2.0, 0.0, 0.0));
        set_target(&mob, &target);

        assert!(!goal.can_use(&mob));
    }

    #[test]
    fn leap_at_target_goal_applies_vanilla_leap_velocity() {
        init_test_registry();
        let mut goal = LeapAtTargetGoal::new(0.42);
        let mob = pig(1, DVec3::ZERO);
        mob.base().set_on_ground(true);
        mob.set_velocity(DVec3::new(1.0, 0.0, 0.0));
        let target = shared_pig(2, DVec3::new(4.0, 0.0, 0.0));
        set_target(&mob, &target);

        goal.target = Some(target);
        goal.start(&mob);

        assert_vec3_close(mob.velocity(), DVec3::new(0.6, f64::from(0.42_f32), 0.0));
    }

    #[test]
    fn leap_at_target_goal_continues_until_grounded() {
        init_test_registry();
        let mut goal = LeapAtTargetGoal::new(0.4);
        let mob = pig(1, DVec3::ZERO);

        assert!(goal.can_continue_to_use(&mob));

        mob.base().set_on_ground(true);
        assert!(!goal.can_continue_to_use(&mob));
    }
}
