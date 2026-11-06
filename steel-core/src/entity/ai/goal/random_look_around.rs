use std::f64::consts::TAU;

use glam::DVec3;

use crate::entity::ai::control::{DEFAULT_LOOK_X_MAX_ROT_ANGLE, DEFAULT_LOOK_Y_MAX_ROT_SPEED};
use crate::entity::ai::goal::selector::{Goal, GoalControls};
use crate::entity::{Mob, SharedEntity};

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

    fn can_use(&mut self, _mob: &Mob<'_>) -> bool {
        rand::random::<f32>() < RANDOM_LOOK_AROUND_CHANCE
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

    fn tick(&mut self, mob: &Mob<'_>, _entity: &SharedEntity) {
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
    use std::sync::Arc;

    use std::sync::Weak;

    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::locks::SyncMutex;

    use super::*;

    use crate::entity::MobParts as _;
    use crate::entity::component::Has;
    use crate::entity::component::definition::MobDefinition;
    use crate::entity::{Entity, EntityBase, LivingEntity, LivingEntityBase, MobBase};
    use crate::entity::{EntitySyncedData, MobProps, mob};
    use std::sync::LazyLock;
    use steel_registry::vanilla_entity_data::MobEntityData;
    use steel_utils::{DowncastType as _, DowncastTypeKey, ErasedType};

    struct TestPathfinderMob {
        base: EntityBase,
        living_base: LivingEntityBase,
        mob_base: MobBase,
        entity_data: SyncMutex<MobEntityData>,
        health: SyncMutex<f32>,
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
                ),
                living_base: LivingEntityBase::new(&vanilla_entities::PIG),
                mob_base: MobBase::new(&TestPathfinderMob::MOB),
                entity_data: SyncMutex::new(MobEntityData::new()),
                health: SyncMutex::new(10.0),
            }
        }
    }

    crate::entity::impl_test_downcast_type!(TestPathfinderMob);

    impl TestPathfinderMob {
        const MOB: MobProps = MobProps {
            pathfinder: true,
            ..MobProps::new(|| TestPathfinderMob::definition())
        };

        fn definition() -> &'static MobDefinition<Self> {
            static DEFINITION: LazyLock<MobDefinition<TestPathfinderMob>> =
                LazyLock::new(mob::stages::definition);
            &DEFINITION
        }
    }

    impl Entity for TestPathfinderMob {
        fn erased_component(&self, key: DowncastTypeKey) -> Option<&dyn ErasedType> {
            (key == MobBase::TYPE_KEY).then_some(&self.mob_base as &dyn ErasedType)
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

    impl Has<MobBase> for TestPathfinderMob {
        fn component(&self) -> &MobBase {
            &self.mob_base
        }
    }

    #[test]
    fn random_look_around_sets_look_control_to_eye_height() {
        let mob = Arc::new(TestPathfinderMob::new());
        let mob_entity: SharedEntity = Arc::<TestPathfinderMob>::clone(&mob);
        let mut goal = RandomLookAroundGoal::new();

        goal.start(&mob.mob());
        goal.tick(&mob.mob(), &mob_entity);

        let look_control = mob.mob().mob_base().controls().lock().look_control;
        assert!(look_control.is_looking_at_target());
        assert_eq!(
            look_control.wanted_position().y.to_bits(),
            mob.get_eye_y().to_bits()
        );
    }
}
