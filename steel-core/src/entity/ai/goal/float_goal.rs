use crate::entity::ai::goal::selector::{Goal, GoalControls};
use crate::entity::{MobBase, PathfinderMob};

const FLOAT_JUMP_CHANCE: f32 = 0.8;

pub struct FloatGoal;

impl FloatGoal {
    #[must_use]
    pub(crate) fn new(mob_base: &MobBase) -> Self {
        mob_base.navigation().lock().set_can_float(true);
        Self
    }
}

impl Goal for FloatGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::JUMP
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        mob.is_in_water() && mob.fluid_contact().water_height() > mob.get_fluid_jump_threshold()
            || mob.is_in_lava()
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        if rand::random::<f32>() < FLOAT_JUMP_CHANCE {
            mob.mob_base().controls().lock().jump_control.jump();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};
    use steel_utils::locks::SyncMutex;

    use super::*;
    use crate::entity::{
        Entity, EntityBase, EntityFluidContact, LivingEntity, LivingEntityBase, Mob,
    };

    struct TestPathfinderMob {
        base: EntityBase,
        living_base: LivingEntityBase,
        mob_base: MobBase,
        mob_flags: SyncMutex<i8>,
        health: SyncMutex<f32>,
    }

    impl TestPathfinderMob {
        fn new(water_height: f64, lava_height: f64) -> Self {
            init_test_registry();
            let base = EntityBase::new(
                1,
                DVec3::ZERO,
                vanilla_entities::PIG.dimensions,
                Weak::new(),
            );
            base.set_fluid_contact(EntityFluidContact::from_parts(
                water_height,
                lava_height,
                false,
                false,
            ));
            Self {
                base,
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
    fn float_goal_sets_navigation_can_float() {
        let mob = TestPathfinderMob::new(0.0, 0.0);

        assert!(!mob.mob_base.navigation().lock().can_float());

        let _goal = FloatGoal::new(&mob.mob_base);

        assert!(mob.mob_base.navigation().lock().can_float());
    }

    #[test]
    fn float_goal_uses_water_threshold_or_lava() {
        let shallow_water_mob = TestPathfinderMob::new(0.3, 0.0);
        let deep_water_mob = TestPathfinderMob::new(0.5, 0.0);
        let lava_mob = TestPathfinderMob::new(0.0, 0.1);
        let mut goal = FloatGoal;

        assert!(!goal.can_use(&shallow_water_mob));
        assert!(goal.can_use(&deep_water_mob));
        assert!(goal.can_use(&lava_mob));
    }
}
