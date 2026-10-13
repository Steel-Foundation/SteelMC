use std::sync::{Arc, Weak};

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_utils::{UuidExt as _, locks::SyncMutex};
use uuid::Uuid;

use crate::world::World;

use super::{SharedEntity, WeakEntity};

/// A UUID reference that re-resolves removed entities within the same domain.
///
/// Its cache is weak to avoid entity ownership cycles.
#[derive(Clone)]
pub struct EntityReference {
    uuid: Uuid,
    // Clones share resolution while callers release their own state locks before lookup.
    cached: Arc<SyncMutex<Option<WeakEntity>>>,
}

impl EntityReference {
    /// Creates an unresolved reference from a persisted UUID.
    #[must_use]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self {
            uuid,
            cached: Arc::new(SyncMutex::new(None)),
        }
    }

    /// Creates a reference with a weak cache of a live entity.
    #[must_use]
    pub fn from_entity(entity: &SharedEntity) -> Self {
        Self {
            uuid: entity.uuid(),
            cached: Arc::new(SyncMutex::new(Some(Arc::downgrade(entity)))),
        }
    }

    /// Reads a UUID reference; absent or malformed values have no reference.
    #[must_use]
    pub fn read(nbt: &BorrowedNbtCompoundView<'_, '_>, key: &str) -> Option<Self> {
        Uuid::from_int_array(&nbt.int_array(key)?).map(Self::from_uuid)
    }

    /// Stores the UUID as an NBT int array.
    pub fn store(&self, nbt: &mut NbtCompound, key: &str) {
        nbt.insert(key, NbtTag::IntArray(self.uuid.to_int_array().to_vec()));
    }

    /// Returns the persistent referenced UUID.
    #[must_use]
    pub const fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Caches an entity only when its UUID matches this reference.
    pub fn cache_entity(&self, entity: &SharedEntity) {
        if entity.uuid() == self.uuid {
            *self.cached.lock() = Some(Arc::downgrade(entity));
        }
    }

    /// Resolves a live cache, then the UUID in any loaded world in this domain.
    #[must_use]
    pub fn get_entity(&self, world: &World) -> Option<SharedEntity> {
        {
            let mut cached = self.cached.lock();
            if let Some(entity) = cached.as_ref().and_then(Weak::upgrade)
                && !entity.is_removed()
                && entity.uuid() == self.uuid
                && entity
                    .level()
                    .is_some_and(|level| level.domain() == world.domain())
            {
                return Some(entity);
            }
            *cached = None;
        }

        let entity = world.get_entity_in_domain_by_uuid(&self.uuid)?;
        if entity.is_removed() || entity.uuid() != self.uuid {
            return None;
        }
        self.cache_entity(&entity);
        Some(entity)
    }
}
