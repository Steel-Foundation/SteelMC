use crate::entity::ai::goal::selector::{Goal, GoalControls};
use crate::entity::{MobBase, PathfinderMob};

const FLOAT_JUMP_CHANCE: f32 = 0.8;

pub struct FloatGoal;

impl FloatGoal {
    #[must_use]
    pub(crate) fn new(mob_base: &mut MobBase) -> Self {
        mob_base.navigation.set_can_float(true);
        Self
    }
}

impl Goal for FloatGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::JUMP
    }

    fn can_use(&mut self, mob: &mut dyn PathfinderMob) -> bool {
        mob.is_in_water() && mob.fluid_contact().water_height() > mob.get_fluid_jump_threshold()
            || mob.is_in_lava()
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &mut dyn PathfinderMob) {
        if rand::random::<f32>() < FLOAT_JUMP_CHANCE {
            mob.mob_base().controls.jump_control.jump();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};
    use steel_utils::locks::SyncMutex;

    use super::*;
    use crate::entity::{
        Entity, EntityBase, EntityFluidContact, LivingEntity, LivingEntityBase, Mob,
    };

    struct TestPathfinderMob {
        base: Weak<EntityBase>,
        living_base: LivingEntityBase,
        mob_base: MobBase,
        mob_flags: SyncMutex<i8>,
        health: SyncMutex<f32>,
    }

    impl TestPathfinderMob {
        fn new(water_height: f64, lava_height: f64) -> Self {
            init_test_registry();
            let base = Arc::new(EntityBase::new(
                1,
                DVec3::ZERO,
                vanilla_entities::PIG.dimensions,
                Weak::new(),
            ));
            base.set_fluid_contact(EntityFluidContact::from_parts(
                water_height,
                lava_height,
                false,
                false,
            ));
            let base_weak = Arc::downgrade(&base);
            // Leak the base so the weak back-reference stays upgradable.
            std::mem::forget(base);
            Self {
                base: base_weak,
                living_base: LivingEntityBase::new(&vanilla_entities::PIG),
                mob_base: MobBase::new(),
                mob_flags: SyncMutex::new(0),
                health: SyncMutex::new(10.0),
            }
        }
    }

    crate::entity::impl_test_downcast_type!(TestPathfinderMob);

    impl Entity for TestPathfinderMob {
        fn base_weak(&self) -> &Weak<EntityBase> {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::PIG
        }
    }

    impl LivingEntity for TestPathfinderMob {
        fn living_base(&mut self) -> &mut LivingEntityBase {
            &mut self.living_base
        }

        fn living_base_ref(&self) -> &LivingEntityBase {
            &self.living_base
        }

        fn get_health(&self) -> f32 {
            *self.health.lock()
        }

        fn set_health(&mut self, health: f32) {
            *self.health.lock() = health;
        }
    }

    impl Mob for TestPathfinderMob {
        fn mob_base(&mut self) -> &mut MobBase {
            &mut self.mob_base
        }

        fn mob_base_ref(&self) -> &MobBase {
            &self.mob_base
        }

        fn mob_flags(&self) -> i8 {
            *self.mob_flags.lock()
        }

        fn set_mob_flags(&mut self, flags: i8) {
            *self.mob_flags.lock() = flags;
        }
    }

    impl PathfinderMob for TestPathfinderMob {}

    #[test]
    fn float_goal_sets_navigation_can_float() {
        let mut mob = TestPathfinderMob::new(0.0, 0.0);

        assert!(!mob.mob_base.navigation.can_float());

        let _goal = FloatGoal::new(&mut mob.mob_base);

        assert!(mob.mob_base.navigation.can_float());
    }

    #[test]
    fn float_goal_uses_water_threshold_or_lava() {
        let mut shallow_water_mob = TestPathfinderMob::new(0.3, 0.0);
        let mut deep_water_mob = TestPathfinderMob::new(0.5, 0.0);
        let mut lava_mob = TestPathfinderMob::new(0.0, 0.1);
        let mut goal = FloatGoal;

        assert!(!goal.can_use(&mut shallow_water_mob));
        assert!(goal.can_use(&mut deep_water_mob));
        assert!(goal.can_use(&mut lava_mob));
    }
}
