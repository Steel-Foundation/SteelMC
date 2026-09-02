//! Proto-chunk entity storage.
//!
//! Full chunks do not own or tick entities. `EntityStorage` only keeps entities
//! staged in proto chunks until promotion hands them to `WorldEntityManager`.

use std::{collections::hash_map::Entry, fmt, mem};

use rustc_hash::FxHashMap;
use steel_utils::locks::SyncRwLock;

use super::{RemovalReason, SharedEntity};

/// Storage for entities staged in a proto chunk.
///
/// Steel keeps proto entity staging separate from full-chunk runtime ownership:
/// promoted or loaded full-chunk entities are owned and ticked by `WorldEntityManager`.
pub(crate) struct EntityStorage {
    state: SyncRwLock<EntityStorageState>,
}

enum EntityStorageState {
    Open(FxHashMap<i32, SharedEntity>),
    Closed,
}

/// Result of trying to stage an entity before full-chunk promotion.
#[must_use]
pub(crate) enum EntityStorageAddResult {
    /// The entity was staged in proto-chunk storage.
    Staged,
    /// Promotion already closed storage, so the caller retains the entity.
    Closed(SharedEntity),
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
            state: SyncRwLock::new(EntityStorageState::Open(FxHashMap::default())),
        }
    }

    /// Creates empty storage that has already crossed the Full promotion boundary.
    #[must_use]
    pub(crate) const fn new_closed() -> Self {
        Self {
            state: SyncRwLock::new(EntityStorageState::Closed),
        }
    }

    /// Tries to add an entity to proto storage.
    ///
    /// This operation linearizes with [`Self::close_and_drain`]. If promotion
    /// closes storage first, ownership is returned so the caller can apply its
    /// phase-specific disposition.
    pub(crate) fn add(&self, entity: SharedEntity) -> EntityStorageAddResult {
        let id = entity.id();
        let mut state = self.state.write();
        let EntityStorageState::Open(entities) = &mut *state else {
            return EntityStorageAddResult::Closed(entity);
        };
        match entities.entry(id) {
            Entry::Vacant(entry) => {
                entry.insert(entity);
                EntityStorageAddResult::Staged
            }
            Entry::Occupied(_) => {
                panic!("entity id {id} is already present in proto entity storage")
            }
        }
    }

    /// Atomically closes proto storage and drains every staged entity.
    ///
    /// Later adds return [`EntityStorageAddResult::Closed`]. Repeated closes
    /// return an empty collection.
    pub(crate) fn close_and_drain(&self) -> Vec<SharedEntity> {
        let mut state = self.state.write();
        let EntityStorageState::Open(entities) =
            mem::replace(&mut *state, EntityStorageState::Closed)
        else {
            return Vec::new();
        };
        entities.into_values().collect()
    }

    /// Returns all staged entities.
    #[must_use]
    pub(crate) fn get_all(&self) -> Vec<SharedEntity> {
        let state = self.state.read();
        let EntityStorageState::Open(entities) = &*state else {
            return Vec::new();
        };
        entities.values().cloned().collect()
    }

    /// Returns the number of staged entities.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        let state = self.state.read();
        match &*state {
            EntityStorageState::Open(entities) => entities.len(),
            EntityStorageState::Closed => 0,
        }
    }

    /// Returns staged entities that should be saved when the proto chunk is persisted.
    ///
    /// Excludes:
    /// - Removed entities
    /// - Entity types with `can_serialize = false` (including players)
    #[must_use]
    pub(crate) fn get_saveable_entities(&self) -> Vec<SharedEntity> {
        let state = self.state.read();
        let EntityStorageState::Open(entities) = &*state else {
            return Vec::new();
        };
        entities
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
    use std::{
        sync::{Arc, Barrier, Weak},
        thread,
    };

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
        assert!(matches!(
            storage.add(unloaded),
            EntityStorageAddResult::Staged
        ));
        assert!(matches!(
            storage.add(discarded),
            EntityStorageAddResult::Staged
        ));

        let saveable = storage.get_saveable_entities();

        assert_eq!(saveable.len(), 1);
        assert_eq!(saveable[0].id(), 1);
    }

    #[test]
    #[should_panic(expected = "already present in proto entity storage")]
    fn add_rejects_duplicate_entity_ids() {
        let storage = EntityStorage::new();

        assert!(matches!(
            storage.add(raw_item(1)),
            EntityStorageAddResult::Staged
        ));
        let _ = storage.add(raw_item(1));
    }

    #[test]
    fn close_drains_staged_entities_and_returns_late_adds() {
        let storage = EntityStorage::new();
        let staged = raw_item(1);
        assert!(matches!(
            storage.add(Arc::clone(&staged)),
            EntityStorageAddResult::Staged
        ));

        let drained = storage.close_and_drain();
        assert_eq!(drained.len(), 1);
        assert!(Arc::ptr_eq(&drained[0], &staged));
        assert!(storage.get_all().is_empty());
        assert!(storage.get_saveable_entities().is_empty());

        let late = raw_item(2);
        let EntityStorageAddResult::Closed(returned) = storage.add(Arc::clone(&late)) else {
            panic!("closed storage must return ownership of a late entity");
        };
        assert!(Arc::ptr_eq(&returned, &late));
        assert!(storage.close_and_drain().is_empty());
    }

    #[test]
    fn concurrent_add_and_close_leave_entity_with_exactly_one_owner() {
        for id in 0..64 {
            let storage = Arc::new(EntityStorage::new());
            let barrier = Arc::new(Barrier::new(2));
            let entity = raw_item(id);
            let add_storage = Arc::clone(&storage);
            let add_barrier = Arc::clone(&barrier);
            let add_entity = Arc::clone(&entity);
            let add_thread = thread::spawn(move || {
                add_barrier.wait();
                add_storage.add(add_entity)
            });

            barrier.wait();
            let drained = storage.close_and_drain();
            let Ok(add_result) = add_thread.join() else {
                panic!("entity staging thread panicked");
            };

            match add_result {
                EntityStorageAddResult::Staged => {
                    assert_eq!(drained.len(), 1);
                    assert!(Arc::ptr_eq(&drained[0], &entity));
                }
                EntityStorageAddResult::Closed(returned) => {
                    assert!(drained.is_empty());
                    assert!(Arc::ptr_eq(&returned, &entity));
                }
            }
            assert!(storage.get_all().is_empty());
        }
    }
}
