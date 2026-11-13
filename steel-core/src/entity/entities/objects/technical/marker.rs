//! Vanilla `Marker`: a server-only position holder for data packs and maps.

use steel_macros::entity_behavior;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_type::EntityTypeRef;

use crate::entity::EntityTable;
use crate::entity::{Entity, EntityBase, EntityProps};

/// A marker: no physics, no ticking, no damage and no passengers. Never sent
/// to clients.
#[entity_behavior(class = "Marker")]
#[derive(Entity)]
#[entity(key = "steel:entity/marker", constructors)]
pub struct MarkerEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
}

impl EntityTable for MarkerEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        // `Marker.tick` does nothing.
        tick: |_| {},
        can_add_passenger: |_, _| false,
        could_accept_passenger: |_| false,
        piston_push_reaction: |_| PushReaction::IgnoreEntity,
        is_ignoring_block_triggers: |_| true,
        ..EntityProps::new()
    };
}

impl MarkerEntity {
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
        MarkerEntity::new(
            &vanilla_entities::MARKER,
            0,
            POSITION,
            Arc::downgrade(test_world()),
        )
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
        let pig: SharedEntity = PigEntity::new(
            &vanilla_entities::PIG,
            1,
            POSITION,
            Arc::downgrade(test_world()),
        );

        assert!(!start_riding_entities(&pig, &marker));
    }
}
