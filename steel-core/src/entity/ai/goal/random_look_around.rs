use crate::entity::prelude::*;
use std::f64::consts::TAU;

use glam::DVec3;

use crate::entity::Mob;
use crate::entity::ai::control::{DEFAULT_LOOK_X_MAX_ROT_ANGLE, DEFAULT_LOOK_Y_MAX_ROT_SPEED};
use crate::entity::ai::goal::selector::{Goal, GoalControls};

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

    fn can_use(&mut self, _mob: &Mob<'_>) -> bool {
        rand::random::<f32>() < 0.02
    }

    fn can_continue_to_use(&mut self, _mob: &Mob<'_>) -> bool {
        self.look_time >= 0
    }

    fn start(&mut self, _mob: &Mob<'_>) {
        let direction = rand::random_range(0.0..TAU);
        self.rel_x = direction.cos();
        self.rel_z = direction.sin();
        self.look_time = 20 + rand::random_range(0..20);
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &Mob<'_>) {
        self.look_time -= 1;
        let position = mob.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(
                position.x + self.rel_x,
                mob.eye_y(),
                position.z + self.rel_z,
            ),
            DEFAULT_LOOK_Y_MAX_ROT_SPEED,
            DEFAULT_LOOK_X_MAX_ROT_ANGLE,
        );
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
    use std::sync::Arc;

    use std::sync::Weak;

    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::locks::CheckedMutex;

    use super::*;

    use crate::entity::EntityProps;
    use crate::entity::component::Has;
    use crate::entity::{Entity, EntityBase, LivingEntityBase, MobBase};
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
        fn new() -> Self {
            init_vanilla_registry();
            Self {
                base: EntityBase::new(
                    1,
                    DVec3::ZERO,
                    vanilla_entities::PIG.dimensions,
                    Weak::new(),
                    WeakSelf::detached(),
                ),
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
    fn random_look_around_sets_look_control_to_eye_height() {
        let mob = Arc::new(TestPathfinderMob::new());
        let mut goal = RandomLookAroundGoal::new();

        goal.start(&mob.mob());
        goal.tick(&mob.mob());

        let look_control = mob.mob().mob_base().controls().lock().look_control;
        assert!(look_control.is_looking_at_target());
        assert_eq!(
            look_control.wanted_position().y.to_bits(),
            mob.eye_y().to_bits()
        );
    }
}
