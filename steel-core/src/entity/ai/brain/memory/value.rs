//! The value side of a brain memory.

use std::fmt::Debug;
use std::sync::Arc;

use steel_utils::{DowncastType, DowncastTypeKey, ErasedType};

use crate::entity::{SharedEntity, WeakEntity};

/// A value a brain memory slot can hold.
///
/// Memories are stored erased as `Box<dyn MemoryValue>` and recovered with
/// Steel's keyed downcasting, so every implementor carries a
/// [`DowncastTypeKey`] through [`ErasedType`].
///
/// A memory's Minecraft registry identity (its
/// [`MemoryModuleType`](super::MemoryModuleType) key) and its Rust value
/// identity (the downcast key) are separate concepts: many memories share one
/// value type, and a value type carries no registry meaning of its own.
///
/// Memories that refer to entities must hold weak references
/// ([`RememberedEntity`], [`RememberedEntities`]). A brain outlives the entities
/// it has seen, so a strong reference would keep a removed entity alive and lets
/// two mobs that remember each other form a cycle.
pub trait MemoryValue: ErasedType + Debug + Send + Sync + 'static {
    /// Returns whether this value is an empty collection.
    ///
    /// Vanilla `Brain.isEmptyCollection`: writing an empty collection clears the
    /// slot instead of storing it, so a
    /// [`ValuePresent`](super::MemoryStatus::ValuePresent) entry condition never
    /// matches an empty list.
    fn is_empty_collection(&self) -> bool {
        false
    }
}

/// A brain memory referring to a single entity.
///
/// Diverges from vanilla, which stores a strong reference and leaves behaviors
/// to test `isAlive()`: here a memory of an entity that has left the world
/// resolves to `None` on its own.
#[derive(Debug, Clone)]
pub struct RememberedEntity(WeakEntity);

// SAFETY: This Steel-owned key uniquely identifies the concrete Rust type within
// the process.
unsafe impl DowncastType for RememberedEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:memory_value/entity");
}

impl MemoryValue for RememberedEntity {}

impl RememberedEntity {
    /// Remembers `entity` without keeping it alive.
    #[must_use]
    pub fn new(entity: &SharedEntity) -> Self {
        Self(Arc::downgrade(entity))
    }

    /// Returns the remembered entity while it is still in the world.
    #[must_use]
    pub fn resolve(&self) -> Option<SharedEntity> {
        self.0.upgrade()
    }
}

impl From<&SharedEntity> for RememberedEntity {
    fn from(entity: &SharedEntity) -> Self {
        Self::new(entity)
    }
}

/// A brain memory referring to several entities, in the order the sensor
/// produced them.
///
/// Entries that have left the world are skipped by [`resolve`](Self::resolve)
/// rather than removed, so the memory stays cheap to read and the list is only
/// rebuilt when its sensor next runs.
#[derive(Debug, Clone, Default)]
pub struct RememberedEntities(Vec<WeakEntity>);

// SAFETY: This Steel-owned key uniquely identifies the concrete Rust type within
// the process.
unsafe impl DowncastType for RememberedEntities {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:memory_value/entities");
}

impl MemoryValue for RememberedEntities {
    fn is_empty_collection(&self) -> bool {
        self.0.is_empty()
    }
}

impl RememberedEntities {
    /// Returns the entities still in the world, in remembered order.
    pub fn resolve(&self) -> impl Iterator<Item = SharedEntity> + '_ {
        self.0.iter().filter_map(WeakEntity::upgrade)
    }

    /// Returns how many entities were remembered, including any that have since
    /// left the world.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether nothing was remembered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'entity> FromIterator<&'entity SharedEntity> for RememberedEntities {
    fn from_iter<I: IntoIterator<Item = &'entity SharedEntity>>(entities: I) -> Self {
        Self(entities.into_iter().map(Arc::downgrade).collect())
    }
}
