//! Container locking utilities for deadlock-free multi-container operations.
//!
//! This module provides types for safely locking multiple containers in a
//! deterministic order to prevent deadlocks when performing operations that
//! span multiple inventories (e.g., transferring items between containers).

use parking_lot::ArcMutexGuard;
use parking_lot::RawMutex;
use rustc_hash::FxHashMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use steel_utils::{Downcast as _, DowncastType, locks::SyncMutex};

use crate::{
    block_entity::{BlockEntity, SharedBlockEntity},
    inventory::container::Container,
    player::player_inventory::PlayerInventory,
};

/// Thread-safe reference to a player inventory.
pub type SyncPlayerInv = Arc<SyncMutex<PlayerInventory>>;

/// Thread-safe reference to an erased container.
pub type SharedContainer = Arc<SyncMutex<dyn Container>>;

enum LockedContainer {
    Container(ArcMutexGuard<RawMutex, dyn Container>),
    BlockEntity(ArcMutexGuard<RawMutex, dyn BlockEntity>),
}

impl Deref for LockedContainer {
    type Target = dyn Container;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Container(guard) => &**guard,
            Self::BlockEntity(guard) => {
                let Some(container) = (**guard).as_container() else {
                    unreachable!("block-entity container reference was validated before locking");
                };
                container
            }
        }
    }
}

impl DerefMut for LockedContainer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Container(guard) => &mut **guard,
            Self::BlockEntity(guard) => {
                let Some(container) = (**guard).as_container_mut() else {
                    unreachable!("block-entity container reference was validated before locking");
                };
                container
            }
        }
    }
}

/// A reference to a container that can be locked.
///
/// This type erases ordinary containers while retaining the separate world-owned
/// block-entity lock source. Use [`ContainerLockGuard::lock_all`] to lock multiple
/// containers in a deadlock-free manner.
#[derive(Clone)]
pub struct ContainerRef {
    id: ContainerId,
    source: ContainerRefSource,
}

#[derive(Clone)]
enum ContainerRefSource {
    Container(SharedContainer),
    BlockEntity(SharedBlockEntity),
}

impl<T> From<Arc<SyncMutex<T>>> for ContainerRef
where
    T: Container + 'static,
{
    fn from(container: Arc<SyncMutex<T>>) -> Self {
        let id = ContainerId::from_arc(&container);
        let container: SharedContainer = container;
        Self {
            id,
            source: ContainerRefSource::Container(container),
        }
    }
}

impl From<SharedContainer> for ContainerRef {
    fn from(container: SharedContainer) -> Self {
        Self {
            id: ContainerId::from_arc(&container),
            source: ContainerRefSource::Container(container),
        }
    }
}

impl ContainerRef {
    /// Creates a `ContainerRef` from a block entity, if it implements Container.
    ///
    /// Returns `None` if the block entity does not implement Container
    /// (i.e., `as_container()` returns `None`).
    #[must_use]
    pub fn from_block_entity(block_entity: SharedBlockEntity) -> Option<Self> {
        let is_container = block_entity.lock().as_container().is_some();
        if !is_container {
            return None;
        }

        Some(Self {
            id: ContainerId::from_arc(&block_entity),
            source: ContainerRefSource::BlockEntity(block_entity),
        })
    }

    /// Returns a unique identifier for this container based on its Arc pointer address.
    #[must_use]
    pub const fn container_id(&self) -> ContainerId {
        self.id
    }

    /// Locks this container and returns a guard.
    fn lock(&self) -> LockedContainer {
        match &self.source {
            ContainerRefSource::Container(arc) => {
                LockedContainer::Container(SyncMutex::lock_arc(arc))
            }
            ContainerRefSource::BlockEntity(arc) => {
                LockedContainer::BlockEntity(SyncMutex::lock_arc(arc))
            }
        }
    }
}

/// A guard that holds locks on multiple containers in a deterministic order.
///
/// This struct ensures that when multiple containers need to be locked simultaneously,
/// they are always locked in the same order (by pointer address) to prevent deadlocks.
///
/// # Example
///
/// ```ignore
/// let player_inv = ContainerRef::from(player_inv_arc);
/// let chest = ContainerRef::from(chest_arc);
///
/// let mut guard = ContainerLockGuard::lock_all(&[&player_inv, &chest]);
///
/// // Access containers by their IDs
/// let player_id = player_inv.container_id();
/// if let Some(inv) = guard.get_mut(player_id) {
///     // Modify the player inventory
/// }
/// ```
pub struct ContainerLockGuard {
    // Store locked guards in deterministic order
    guards: Vec<(ContainerId, LockedContainer)>,
    // For quick lookup
    id_to_index: FxHashMap<ContainerId, usize>,
}

impl ContainerLockGuard {
    /// Create a new lock guard and lock all containers in deterministic order.
    ///
    /// Containers are sorted by their pointer address before locking to ensure
    /// a consistent lock order across all call sites, preventing deadlocks.
    /// Duplicate containers (same Arc) are automatically deduplicated.
    #[must_use]
    pub fn lock_all(containers: &[&ContainerRef]) -> Self {
        // Collect container IDs and references, then sort
        let mut to_lock: Vec<_> = containers.iter().map(|c| (c.container_id(), *c)).collect();

        // Sort by ID for deterministic lock order (prevents deadlocks)
        to_lock.sort_by_key(|(id, _)| *id);

        // Deduplicate (in case same container passed multiple times)
        to_lock.dedup_by_key(|(id, _)| *id);

        // Lock all in sorted order
        let mut guards = Vec::with_capacity(to_lock.len());
        for (id, container) in to_lock {
            let guard = container.lock();
            guards.push((id, guard));
        }

        // Build index map
        let id_to_index = guards
            .iter()
            .enumerate()
            .map(|(idx, (id, _))| (*id, idx))
            .collect();

        Self {
            guards,
            id_to_index,
        }
    }

    /// Unlock all containers and relock with a new set.
    ///
    /// This should only be called when you need to add more containers
    /// mid-operation. All existing references from `get()`/`get_mut()` are invalidated.
    #[must_use]
    pub fn relock(self, containers: &[&ContainerRef]) -> Self {
        // Drop self, releasing all locks
        drop(self);
        // Lock new set
        Self::lock_all(containers)
    }

    /// Get immutable access to a locked container.
    #[must_use]
    pub fn get(&self, id: impl Into<ContainerId>) -> Option<&dyn Container> {
        self.id_to_index
            .get(&id.into())
            .and_then(|&idx| self.guards.get(idx))
            .map(|(_, guard)| &**guard as &dyn Container)
    }

    /// Get mutable access to a locked container.
    pub fn get_mut(&mut self, id: impl Into<ContainerId>) -> Option<&mut dyn Container> {
        self.id_to_index
            .get(&id.into())
            .copied()
            .and_then(|idx| self.guards.get_mut(idx))
            .map(|(_, guard)| &mut **guard as &mut dyn Container)
    }

    /// Gets immutable access when the locked container has concrete type `T`.
    #[must_use]
    pub fn get_typed<T>(&self, id: impl Into<ContainerId>) -> Option<&T>
    where
        T: Container + DowncastType,
    {
        self.get(id)?.downcast_ref::<T>()
    }

    /// Gets mutable access when the locked container has concrete type `T`.
    pub fn get_typed_mut<T>(&mut self, id: impl Into<ContainerId>) -> Option<&mut T>
    where
        T: Container + DowncastType,
    {
        self.get_mut(id)?.downcast_mut::<T>()
    }

    /// Check if a container is locked.
    #[must_use]
    pub fn contains(&self, id: ContainerId) -> bool {
        self.id_to_index.contains_key(&id)
    }
}

/// Unique identifier for a container based on Arc pointer address.
///
/// This ID is used to establish a deterministic ordering when locking
/// multiple containers, preventing deadlocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContainerId(usize);

impl ContainerId {
    /// Creates a container ID from an Arc's pointer address.
    pub fn from_arc<T: ?Sized>(arc: &Arc<T>) -> Self {
        Self(Arc::as_ptr(arc).cast::<()>() as usize)
    }
}

impl From<&SyncPlayerInv> for ContainerId {
    fn from(value: &SyncPlayerInv) -> Self {
        Self::from_arc(value)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use steel_registry::vanilla_block_entity_types;
    use steel_utils::{BlockPos, BlockStateId, locks::SyncMutex};

    use super::{ContainerId, ContainerLockGuard, ContainerRef};
    use crate::block_entity::{
        SharedBlockEntity,
        entities::{BarrelBlockEntity, RawBlockEntity},
    };
    use crate::inventory::crafting::{CraftingContainer, ResultContainer};

    #[test]
    fn erased_container_ref_preserves_id_and_typed_access() {
        let crafting = Arc::new(SyncMutex::new(CraftingContainer::new(2, 2)));
        let id = ContainerId::from_arc(&crafting);
        let container_ref = ContainerRef::from(Arc::clone(&crafting));

        assert_eq!(container_ref.container_id(), id);

        let mut guard = ContainerLockGuard::lock_all(&[&container_ref]);
        let Some(typed) = guard.get_typed::<CraftingContainer>(id) else {
            panic!("erased crafting container should retain its concrete type");
        };
        assert_eq!((typed.width(), typed.height()), (2, 2));
        assert!(guard.get_typed::<ResultContainer>(id).is_none());
        assert!(guard.get_typed_mut::<CraftingContainer>(id).is_some());
    }

    #[test]
    fn validated_block_entity_ref_supports_typed_container_access() {
        let barrel = Arc::new(SyncMutex::new(BarrelBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            BlockStateId::default(),
        )));
        let id = ContainerId::from_arc(&barrel);
        let block_entity: SharedBlockEntity = barrel.clone();
        let Some(container_ref) = ContainerRef::from_block_entity(block_entity) else {
            panic!("barrel block entity should expose Container");
        };

        assert_eq!(container_ref.container_id(), id);

        let guard = ContainerLockGuard::lock_all(&[&container_ref]);
        assert!(guard.get_typed::<BarrelBlockEntity>(id).is_some());
    }

    #[test]
    fn non_container_block_entity_ref_is_rejected() {
        let block_entity: SharedBlockEntity = Arc::new(SyncMutex::new(RawBlockEntity::new(
            &vanilla_block_entity_types::END_PORTAL,
            Weak::new(),
            BlockPos::new(1, 2, 3),
            BlockStateId::default(),
        )));

        assert!(ContainerRef::from_block_entity(block_entity).is_none());
    }
}
