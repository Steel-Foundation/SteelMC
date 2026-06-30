use std::f64::consts::TAU;

use glam::DVec3;

use crate::entity::PathfinderMob;
use crate::entity::ai::control::{DEFAULT_LOOK_X_MAX_ROT_ANGLE, DEFAULT_LOOK_Y_MAX_ROT_SPEED};
use crate::entity::ai::goal::selector::{Goal, GoalControls};

const RANDOM_LOOK_AROUND_CHANCE: f32 = 0.02;

pub struct RandomLookAroundGoal {
    rel_x: f64,
    rel_z: f64,
    look_time: i32,
}

impl RandomLookAroundGoal {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            rel_x: 0.0,
            rel_z: 0.0,
            look_time: 0,
        }
    }
}

impl Default for RandomLookAroundGoal {
    fn default() -> Self {
        Self::new()
    }
}

impl Goal for RandomLookAroundGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, _mob: &dyn PathfinderMob) -> bool {
        rand::random::<f32>() < RANDOM_LOOK_AROUND_CHANCE
    }

    fn can_continue_to_use(&mut self, _mob: &dyn PathfinderMob) -> bool {
        self.look_time >= 0
    }

    fn start(&mut self, _mob: &dyn PathfinderMob) {
        let direction = TAU * rand::random::<f64>();
        self.rel_x = direction.cos();
        self.rel_z = direction.sin();
        self.look_time = 20 + rand::random_range(0..20);
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.look_time -= 1;
        let position = mob.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(
                position.x + self.rel_x,
                mob.get_eye_y(),
                position.z + self.rel_z,
            ),
            DEFAULT_LOOK_Y_MAX_ROT_SPEED,
            DEFAULT_LOOK_X_MAX_ROT_ANGLE,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};
    use steel_utils::locks::SyncMutex;

    use super::*;
    use crate::entity::{Entity, EntityBase, LivingEntity, LivingEntityBase, Mob, MobBase};

    struct TestPathfinderMob {
        base: EntityBase,
        living_base: LivingEntityBase,
        mob_base: MobBase,
        mob_flags: SyncMutex<i8>,
        health: SyncMutex<f32>,
    }

    impl TestPathfinderMob {
        fn new() -> Self {
            init_test_registry();
            Self {
                base: EntityBase::new(
                    1,
                    DVec3::ZERO,
                    vanilla_entities::PIG.dimensions,
                    Weak::new(),
                ),
                living_base: LivingEntityBase::new(&vanilla_entities::PIG),
                mob_base: MobBase::new(),
                mob_flags: SyncMutex::new(0),
                health: SyncMutex::new(10.0),
            }
        }
    }

    impl Entity for TestPathfinderMob {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::PIG
        }
    }

    impl LivingEntity for TestPathfinderMob {
        fn living_base(&self) -> &LivingEntityBase {
            &self.living_base
        }

        fn get_health(&self) -> f32 {
            *self.health.lock()
        }

        fn set_health(&self, health: f32) {
            *self.health.lock() = health;
        }
    }

    impl Mob for TestPathfinderMob {
        fn mob_base(&self) -> &MobBase {
            &self.mob_base
        }

        fn mob_flags(&self) -> i8 {
            *self.mob_flags.lock()
        }

        fn set_mob_flags(&self, flags: i8) {
            *self.mob_flags.lock() = flags;
        }
    }

    impl PathfinderMob for TestPathfinderMob {}

    #[test]
    fn random_look_around_sets_look_control_to_eye_height() {
        let mob = TestPathfinderMob::new();
        let mut goal = RandomLookAroundGoal::new();

        goal.start(&mob);
        goal.tick(&mob);

        let look_control = mob.mob_base().controls().lock().look_control;
        assert!(look_control.is_looking_at_target());
        assert_eq!(
            look_control.wanted_position().y.to_bits(),
            mob.get_eye_y().to_bits()
        );
    }
}
