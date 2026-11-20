//! Fixtures for benchmarking the world entity manager without a live world.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entities;
use steel_utils::ChunkPos;
use uuid::Uuid;

use crate::entity::{Entity, EntityBase, SharedEntity};

use super::{EntityOwnership, EntityVisibility, WorldEntityManager};

const CHUNK_WIDTH: i32 = 16;
const ENTITY_Y: f64 = 64.0;
const BLOCK_CENTER: f64 = 0.5;

struct BenchEntity {
    base: EntityBase,
}

impl BenchEntity {
    fn shared(id: i32, position: DVec3) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::with_uuid(
                id,
                Uuid::from_u128(u128::try_from(id).expect("positive benchmark entity id") + 1),
                position,
                vanilla_entities::ITEM.dimensions,
                Weak::new(),
            ),
        })
    }
}

// SAFETY: The key is unique to this benchmark-only type in the test process.
unsafe impl steel_utils::DowncastType for BenchEntity {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:bench/entity_manager/BenchEntity");
}

impl Entity for BenchEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }
}

/// Builds a manager with `count` ticking, non-riding entities in one chunk.
///
/// # Panics
///
/// Panics if `count` exceeds `i32::MAX` or an entity fails to register.
#[must_use]
pub fn manager_with_entities(count: usize) -> WorldEntityManager {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    let _ = manager.on_chunk_loaded(chunk);
    let _ = manager.update_chunk_visibility(chunk, EntityVisibility::Ticking);

    for index in 0..count {
        let index = i32::try_from(index).expect("benchmark entity count fits in i32");
        let entity = BenchEntity::shared(
            index + 1,
            DVec3::new(
                f64::from(index % CHUNK_WIDTH) + BLOCK_CENTER,
                ENTITY_Y,
                f64::from((index / CHUNK_WIDTH) % CHUNK_WIDTH) + BLOCK_CENTER,
            ),
        );
        manager
            .add_live_entity(entity, EntityOwnership::ManagerOwned)
            .expect("benchmark entity should register");
    }

    manager
}
