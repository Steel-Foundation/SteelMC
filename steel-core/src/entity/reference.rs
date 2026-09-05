use std::fmt::{self, Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Weak};

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_utils::UuidExt as _;
use steel_utils::locks::SyncMutex;
use uuid::Uuid;

use crate::world::World;

use super::{Entity, SharedEntity, WeakEntity};

/// A persistent entity UUID with a weak cache of its currently loaded entity.
///
/// Resolution is restricted to loaded worlds in the requesting world's Steel
/// domain. The weak cache avoids ownership cycles while preserving Vanilla's
/// ability to retain a live reference across same-domain world transitions.
#[derive(Clone)]
pub struct EntityReference {
    uuid: Uuid,
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

    /// Reads a Vanilla UUID-backed entity reference from NBT.
    #[must_use]
    pub fn read(nbt: &BorrowedNbtCompoundView<'_, '_>, key: &str) -> Option<Self> {
        let value = nbt.int_array(key)?;
        Uuid::from_int_array(&value).map(Self::from_uuid)
    }

    /// Stores this reference using Vanilla's UUID int-array representation.
    pub fn store(&self, nbt: &mut NbtCompound, key: impl Into<String>) {
        nbt.insert(
            key.into(),
            NbtTag::IntArray(self.uuid.to_int_array().to_vec()),
        );
    }

    /// Returns the persistent referenced UUID.
    #[must_use]
    pub const fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Returns whether this reference identifies `entity`.
    #[must_use]
    pub fn matches(&self, entity: &dyn Entity) -> bool {
        self.uuid == entity.uuid()
    }

    /// Caches `entity` when its UUID matches this reference.
    pub fn cache_entity(&self, entity: &SharedEntity) {
        if self.matches(entity.as_ref()) {
            *self.cached.lock() = Some(Arc::downgrade(entity));
        }
    }

    /// Resolves an entity in any loaded world within `world`'s Steel domain.
    #[must_use]
    pub fn get_entity(&self, world: &World) -> Option<SharedEntity> {
        self.get_matching(world, |_| true)
    }

    /// Resolves a living entity in any loaded world within `world`'s Steel domain.
    #[must_use]
    pub fn get_living_entity(&self, world: &World) -> Option<SharedEntity> {
        self.get_matching(world, Entity::is_living_entity)
    }

    /// Resolves the exact entity retained by a [`crate::entity::damage::DamageSource`].
    ///
    /// Vanilla damage sources retain their entity objects even after removal. A matching
    /// same-domain cached entity therefore remains authoritative here, while an expired cache
    /// falls back to the normal loaded-domain UUID lookup.
    #[must_use]
    pub(crate) fn get_damage_source_entity(&self, world: &World) -> Option<SharedEntity> {
        {
            let cached = self.cached.lock();
            if let Some(entity) = cached.as_ref().and_then(Weak::upgrade)
                && self.matches(entity.as_ref())
                && entity
                    .level()
                    .is_some_and(|level| level.domain() == world.domain())
            {
                return Some(entity);
            }
        }

        self.get_entity(world)
    }

    fn get_matching(
        &self,
        world: &World,
        matches_type: impl Fn(&dyn Entity) -> bool,
    ) -> Option<SharedEntity> {
        {
            let mut cached = self.cached.lock();
            if let Some(entity) = cached.as_ref().and_then(Weak::upgrade)
                && !entity.is_removed()
                && self.matches(entity.as_ref())
                && entity
                    .level()
                    .is_some_and(|level| level.domain() == world.domain())
                && matches_type(entity.as_ref())
            {
                return Some(entity);
            }
            *cached = None;
        }

        let entity = world.get_entity_in_domain_by_uuid(&self.uuid)?;
        if entity.is_removed() || !matches_type(entity.as_ref()) {
            return None;
        }
        *self.cached.lock() = Some(Arc::downgrade(&entity));
        Some(entity)
    }
}

impl Debug for EntityReference {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EntityReference")
            .field("uuid", &self.uuid)
            .finish_non_exhaustive()
    }
}

impl PartialEq for EntityReference {
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
    }
}

impl Eq for EntityReference {}

impl Hash for EntityReference {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.uuid.hash(state);
    }
}
