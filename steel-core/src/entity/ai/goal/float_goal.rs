use crate::entity::ai::goal::selector::{Goal, GoalControls};
use crate::entity::prelude::*;
use crate::entity::{Mob, MobBase};

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

    fn can_use(&mut self, mob: &Mob<'_>) -> bool {
        mob.is_in_water() && mob.fluid_contact().water_height() > mob.fluid_jump_threshold()
            || mob.is_in_lava()
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &Mob<'_>) {
        if rand::random::<f32>() < 0.8 {
            mob.mob_base().controls().lock().jump_control.jump();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::EntityTable;
    use crate::entity::LivingProps;
    use crate::entity::LivingTable;
    use crate::entity::MobTable;
    use crate::entity::WeakSelf;
    use crate::test_support::synced_with_health;
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::locks::CheckedMutex;

    use super::*;
    use crate::entity::EntityProps;
    use crate::entity::component::Has;
    use crate::entity::{Entity, EntityBase, EntityFluidContact, LivingEntityBase};
    use crate::entity::{EntitySyncedData, MobProps, mob};
    use steel_registry::vanilla_entity_data::MobEntityData;
    use steel_utils::{DowncastType as _, DowncastTypeKey, ErasedType};

    struct TestPathfinderMob {
        base: EntityBase,
        living_base: LivingEntityBase,
        mob_base: MobBase,
        entity_data: CheckedMutex<MobEntityData>,
    }

    impl TestPathfinderMob {
        fn new(water_height: f64, lava_height: f64) -> Self {
            init_vanilla_registry();
            let base = EntityBase::new(
                1,
                DVec3::ZERO,
                vanilla_entities::PIG.dimensions,
                Weak::new(),
                WeakSelf::detached(),
            );
            base.set_fluid_contact(EntityFluidContact::from_parts(
                water_height,
                lava_height,
                false,
                false,
            ));
            Self {
                base,
                living_base: LivingEntityBase::without_synced_data(&vanilla_entities::PIG),
                mob_base: MobBase::new(|_| {}),
                entity_data: CheckedMutex::new(synced_with_health(MobEntityData::new(), 10.0)),
            }
        }
    }

    crate::entity::impl_test_downcast_type!(TestPathfinderMob);

    impl MobTable for TestPathfinderMob {
        const MOB: MobProps<Self> = MobProps {
            is_pathfinder: |_| true,
            ..MobProps::new()
        };
    }

    impl TestPathfinderMob {
        const ENTITY: EntityProps<Self> = EntityProps {
            ..mob::entity_props()
        };
    }

    impl Entity for TestPathfinderMob {
        fn erased_component(&self, key: DowncastTypeKey) -> Option<&dyn ErasedType> {
            match key {
                MobBase::TYPE_KEY => Some(&self.mob_base),
                LivingEntityBase::TYPE_KEY => Some(&self.living_base),
                _ => None,
            }
        }

        fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
            Some(&self.entity_data)
        }

        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::PIG
        }
    }

    impl EntityTable for TestPathfinderMob {
        const ENTITY: EntityProps<Self> = Self::ENTITY;
    }

    impl LivingTable for TestPathfinderMob {
        const LIVING: LivingProps<Self> = LivingProps {
            // Tests drive the goals and navigation themselves.
            server_ai_step: |_| {},
            ..mob::living_props()
        };
    }

    impl Has<LivingEntityBase> for TestPathfinderMob {
        fn component(&self) -> &LivingEntityBase {
            &self.living_base
        }
    }

    impl Has<MobBase> for TestPathfinderMob {
        fn component(&self) -> &MobBase {
            &self.mob_base
        }
    }

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
        lava_mob.set_first_tick(false);
        let mut goal = FloatGoal;

        assert!(!goal.can_use(&shallow_water_mob.mob()));
        assert!(goal.can_use(&deep_water_mob.mob()));
        assert!(goal.can_use(&lava_mob.mob()));
    }
}
