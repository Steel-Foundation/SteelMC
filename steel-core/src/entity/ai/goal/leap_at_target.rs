use crate::entity::prelude::*;
use glam::DVec3;

use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use crate::entity::{Mob, SharedEntity};

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

    fn can_use(&mut self, mob: &Mob<'_>) -> bool {
        if mob.has_controlling_passenger() {
            return false;
        }

        let Some(target) = mob.target() else {
            return false;
        };

        let distance_sqr = mob.position().distance_squared(target.position());
        if !(4.0..=16.0).contains(&distance_sqr) {
            return false;
        }

        if !mob.on_ground() {
            return false;
        }

        if rand::random_range(0..reduced_tick_delay(5)) != 0 {
            return false;
        }

        self.target = Some(target);
        true
    }

    fn can_continue_to_use(&mut self, mob: &Mob<'_>) -> bool {
        !mob.on_ground()
    }

    fn start(&mut self, mob: &Mob<'_>) {
        let Some(target) = &self.target else {
            return;
        };

        let movement = mob.velocity();
        let mut delta = target.position() - mob.position();
        delta.y = 0.0;
        if delta.length_squared() > 1.0e-7 {
            delta = delta.normalize() * 0.4 + movement * 0.2;
        }

        mob.set_velocity(DVec3::new(delta.x, f64::from(self.yd), delta.z));
    }

    fn stop(&mut self, _mob: &Mob<'_>) {
        self.target = None;
    }
}

#[cfg(test)]
mod tests {

    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::*;

    use crate::entity::{Entity, entities::ChickenEntity};

    fn pig(id: i32, position: DVec3) -> ChickenEntity {
        ChickenEntity::new(&vanilla_entities::CHICKEN, id, position, Weak::new())
    }

    fn shared_pig(id: i32, position: DVec3) -> SharedEntity {
        Arc::new(pig(id, position))
    }

    fn set_target(mob: &ChickenEntity, target: &SharedEntity) {
        assert!(mob.mob().set_target(Some(target)));
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
        init_vanilla_registry();
        let mut goal = LeapAtTargetGoal::new(0.4);
        let mob = pig(1, DVec3::ZERO);
        mob.base().set_on_ground(true);

        assert!(!goal.can_use(&mob.mob()));
    }

    #[test]
    fn leap_at_target_goal_uses_vanilla_distance_window() {
        init_vanilla_registry();
        let mut goal = LeapAtTargetGoal::new(0.4);
        let mob = pig(1, DVec3::ZERO);
        mob.base().set_on_ground(true);

        let close_target = shared_pig(2, DVec3::new(1.0, 0.0, 0.0));
        set_target(&mob, &close_target);
        assert!(!goal.can_use(&mob.mob()));

        let far_target = shared_pig(3, DVec3::new(5.0, 0.0, 0.0));
        set_target(&mob, &far_target);
        assert!(!goal.can_use(&mob.mob()));
    }

    #[test]
    fn leap_at_target_goal_requires_ground() {
        init_vanilla_registry();
        let mut goal = LeapAtTargetGoal::new(0.4);
        let mob = pig(1, DVec3::ZERO);
        let target = shared_pig(2, DVec3::new(2.0, 0.0, 0.0));
        set_target(&mob, &target);

        assert!(!goal.can_use(&mob.mob()));
    }

    #[test]
    fn leap_at_target_goal_applies_vanilla_leap_velocity() {
        init_vanilla_registry();
        let mut goal = LeapAtTargetGoal::new(0.42);
        let mob = pig(1, DVec3::ZERO);
        mob.base().set_on_ground(true);
        mob.set_velocity(DVec3::new(1.0, 0.0, 0.0));
        let target = shared_pig(2, DVec3::new(4.0, 0.0, 0.0));
        set_target(&mob, &target);

        goal.target = Some(target);
        goal.start(&mob.mob());

        assert_vec3_close(mob.velocity(), DVec3::new(0.6, f64::from(0.42_f32), 0.0));
    }

    #[test]
    fn leap_at_target_goal_continues_until_grounded() {
        init_vanilla_registry();
        let mut goal = LeapAtTargetGoal::new(0.4);
        let mob = pig(1, DVec3::ZERO);

        assert!(goal.can_continue_to_use(&mob.mob()));

        mob.base().set_on_ground(true);
        assert!(!goal.can_continue_to_use(&mob.mob()));
    }
}
