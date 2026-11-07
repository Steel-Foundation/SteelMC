//! Vanilla `Marker`: a server-only position holder for data packs and maps.

use std::sync::Weak;

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_type::EntityTypeRef;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntityProps};
use crate::world::World;

/// A marker: no physics, no ticking, no damage and no passengers. Never sent
/// to clients.
#[entity_behavior(class = "Marker")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY)]
pub struct MarkerEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
}

// SAFETY: This key is owned by Steel and uniquely identifies `MarkerEntity`.
unsafe impl DowncastType for MarkerEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/marker");
}

impl MarkerEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: |_, _| {},
        hurt: |_, _, _, _| false,
        can_add_passenger: |_, _| false,
        could_accept_passenger: |_| false,
        piston_push_reaction: |_| PushReaction::IgnoreEntity,
        is_ignoring_block_triggers: |_| true,
        ..EntityProps::new()
    };

    /// Creates a new marker.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::from_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a marker from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        base.set_no_physics(true);
        Self { base, entity_type }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_damage_types, vanilla_entities};

    use crate::entity::damage::DamageSource;
    use crate::entity::entities::{MarkerEntity, PigEntity};
    use crate::entity::prelude::*;
    use crate::entity::{SharedEntity, start_riding_entities};
    use crate::test_support::test_world;

    const POSITION: DVec3 = DVec3::new(-8.0, 128.0, 4.0);

    fn marker() -> Arc<MarkerEntity> {
        Arc::new(MarkerEntity::new(
            &vanilla_entities::MARKER,
            0,
            POSITION,
            Arc::downgrade(test_world()),
        ))
    }

    #[test]
    fn markers_stay_put_and_cannot_be_hurt() {
        init_vanilla_registry();
        let marker = marker();
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        assert!(!marker.hurt(test_world(), &source, 5.0));
        for _ in 0..100 {
            Arc::<MarkerEntity>::clone(&marker).tick();
        }
        assert_eq!(marker.position(), POSITION);
    }

    #[test]
    fn markers_cannot_carry_passengers() {
        init_vanilla_registry();
        let marker: SharedEntity = marker();
        let pig: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            1,
            POSITION,
            Arc::downgrade(test_world()),
        ));

        assert!(!start_riding_entities(&pig, &marker));
    }
}
