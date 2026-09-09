use crate::entity::damage::DamageSource;
use crate::entity::{Entity, EntityBase, EntityBaseLoad};
use crate::world::World;
use glam::DVec3;
use std::sync::Weak;
use steel_macros::entity_behavior;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_type::EntityTypeRef;
use steel_utils::{DowncastType, DowncastTypeKey};
use uuid::Uuid;

/// An entity that is minimal. It cannot move, take damage, make sounds, and it doesn't
/// have any gameplay functions. Markers are not sent to the client, and are primarily intended for use
/// in map-making and in data packs. This entity is purely server-side.
#[entity_behavior(class = "Marker")]
pub struct MarkerEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
}

// SAFETY: This key is owned by Steel and uniquely identifies `MarkerEntity`.
unsafe impl DowncastType for MarkerEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/marker");
}

impl MarkerEntity {
    /// Creates a new marker entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        }
    }

    /// Creates a new marker entity with a specific UUID.
    #[must_use]
    pub fn with_uuid(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        uuid: Uuid,
        world: Weak<World>,
    ) -> Self {
        Self {
            base: EntityBase::with_uuid(id, uuid, position, entity_type.dimensions, world),
            entity_type,
        }
    }

    /// Creates a marker entity from saved data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        }
    }
}

impl Entity for MarkerEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn is_ignoring_block_triggers(&self) -> bool {
        true
    }

    fn piston_push_reaction(&self) -> PushReaction {
        PushReaction::Ignore
    }

    fn can_add_passenger(&self, _passenger: &dyn Entity) -> bool {
        false
    }

    fn could_accept_passenger(&self) -> bool {
        false
    }

    fn tick(&self) {}

    fn no_physics(&self) -> bool {
        true
    }

    fn hurt(&self, _world: &World, _source: &DamageSource, _amount: f32) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::damage::DamageSource;
    use crate::entity::entities::{MarkerEntity, PigEntity};
    use crate::entity::{Entity, SharedEntity, start_riding_entities};
    use crate::test_support::test_world;
    use glam::DVec3;
    use std::sync::Arc;
    use steel_registry::{vanilla_damage_types, vanilla_entities};

    const TEST_POSITION: DVec3 = DVec3::new(-8.0, 128.0, 4.0);

    #[test]
    fn markers_cannot_get_hurt() {
        let world = test_world();
        let marker = MarkerEntity::new(
            &vanilla_entities::MARKER,
            0,
            TEST_POSITION,
            Arc::downgrade(world),
        );
        assert!(!marker.hurt(
            world,
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            5.0
        ));
    }

    #[test]
    fn markers_cannot_fall() {
        let world = test_world();
        let marker = MarkerEntity::new(
            &vanilla_entities::MARKER,
            0,
            TEST_POSITION,
            Arc::downgrade(world),
        );
        for _ in 0..100 {
            marker.tick();
        }
        assert_eq!(marker.position(), TEST_POSITION);
    }

    #[test]
    fn markers_cannot_have_passenger() {
        let world = test_world();
        let marker: SharedEntity = Arc::new(MarkerEntity::new(
            &vanilla_entities::MARKER,
            0,
            TEST_POSITION,
            Arc::downgrade(world),
        ));
        let passenger: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            1,
            TEST_POSITION,
            Arc::downgrade(world),
        ));
        assert!(!start_riding_entities(&passenger, &marker));
    }
}
