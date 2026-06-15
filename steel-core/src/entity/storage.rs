//! Proto-chunk entity storage.
//!
//! Full chunks do not own or tick entities. `EntityStorage` only keeps entities
//! staged in proto chunks until promotion hands them to `WorldEntityManager`.

use std::fmt;

use rustc_hash::FxHashMap;
use steel_utils::locks::SyncRwLock;

use super::{RemovalReason, SharedEntity};

/// Storage for entities staged in a proto chunk.
///
/// Steel keeps proto entity staging separate from full-chunk runtime ownership:
/// promoted or loaded full-chunk entities are owned and ticked by `WorldEntityManager`.
pub(crate) struct EntityStorage {
    /// Proto-staged entities keyed by entity ID.
    entities: SyncRwLock<FxHashMap<i32, SharedEntity>>,
}

fn should_keep_for_save(entity: &SharedEntity) -> bool {
    !entity.is_removed()
        || entity
            .removal_reason()
            .is_some_and(RemovalReason::should_save)
}

impl fmt::Debug for EntityStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EntityStorage")
            .field("len", &self.len())
            .finish()
    }
}

impl EntityStorage {
    /// Creates a new empty entity storage.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            entities: SyncRwLock::new(FxHashMap::default()),
        }
    }

    /// Adds an entity to proto storage.
    pub(crate) fn add(&self, entity: SharedEntity) {
        let id = entity.id();
        assert!(
            self.entities.write().insert(id, entity).is_none(),
            "entity id {id} is already present in proto entity storage"
        );
    }

    /// Returns all staged entities.
    #[must_use]
    pub(crate) fn get_all(&self) -> Vec<SharedEntity> {
        self.entities.read().values().cloned().collect()
    }

    /// Returns the number of staged entities.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.entities.read().len()
    }

    /// Returns staged entities that should be saved when the proto chunk is persisted.
    ///
    /// Excludes:
    /// - Removed entities
    /// - Entity types with `can_serialize = false` (including players)
    #[must_use]
    pub(crate) fn get_saveable_entities(&self) -> Vec<SharedEntity> {
        self.entities
            .read()
            .values()
            .filter(|e| should_keep_for_save(e) && e.entity_type().can_serialize)
            .cloned()
            .collect()
    }
}

impl Default for EntityStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::vanilla_entities;

    use super::*;
    use crate::entity::entities::RawEntity;

    fn raw_item(id: i32) -> SharedEntity {
        Arc::new(RawEntity::new(
            id,
            DVec3::ZERO,
            Weak::new(),
            &vanilla_entities::ITEM,
        ))
    }

    #[test]
    fn saveable_entities_keep_unloaded_to_chunk_removals() {
        let storage = EntityStorage::new();
        let unloaded = raw_item(1);
        let discarded = raw_item(2);

        unloaded.set_removed(RemovalReason::UnloadedToChunk);
        discarded.set_removed(RemovalReason::Discarded);
        storage.add(unloaded);
        storage.add(discarded);

        let saveable = storage.get_saveable_entities();

        assert_eq!(saveable.len(), 1);
        assert_eq!(saveable[0].id(), 1);
    }

    #[test]
    #[should_panic(expected = "already present in proto entity storage")]
    fn add_rejects_duplicate_entity_ids() {
        let storage = EntityStorage::new();

        storage.add(raw_item(1));
        storage.add(raw_item(1));
    }
}
