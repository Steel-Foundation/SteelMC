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
use steel_utils::locks::SyncMutex;

use crate::{
    block_entity::{BlockEntity, SharedBlockEntity},
    inventory::{
        container::Container,
        crafting::{CraftingContainer, ResultContainer},
    },
    player::player_inventory::PlayerInventory,
};

/// Thread-safe reference to a player inventory.
pub type SyncPlayerInv = Arc<SyncMutex<PlayerInventory>>;

/// Thread-safe reference to a generic container implementing the Container trait.
pub type GenericContainer = Arc<SyncMutex<dyn Container + Send + Sync>>;

/// A locked container guard that provides access to the underlying container.
///
/// This enum wraps different container types with their mutex guards, allowing
/// uniform access through the `Container` trait via `Deref`/`DerefMut`.
pub enum LockedContainer {
    /// A locked player inventory.
    PlayerInventory(ArcMutexGuard<RawMutex, PlayerInventory>),
    /// A locked crafting grid container.
    CraftingContainer(ArcMutexGuard<RawMutex, CraftingContainer>),
    /// A locked crafting result container.
    ResultContainer(ArcMutexGuard<RawMutex, ResultContainer>),
    /// A locked generic container.
    Other(ArcMutexGuard<RawMutex, dyn Container + Send + Sync>),
    /// A locked block entity that implements Container.
    ///
    /// The block entity is guaranteed to return `Some` from `as_container()`
    /// because this variant is only constructed after validation.
    BlockEntity(ArcMutexGuard<RawMutex, dyn BlockEntity>),
}

impl Deref for LockedContainer {
    type Target = dyn Container;

    fn deref(&self) -> &Self::Target {
        match self {
            LockedContainer::PlayerInventory(guard) => &**guard,
            LockedContainer::CraftingContainer(guard) => &**guard,
            LockedContainer::ResultContainer(guard) => &**guard,
            LockedContainer::Other(guard) => &**guard,
            LockedContainer::BlockEntity(guard) => (**guard)
                .as_container()
                .expect("BlockEntity variant should only be used for container block entities"),
        }
    }
}

impl DerefMut for LockedContainer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            LockedContainer::PlayerInventory(guard) => &mut **guard,
            LockedContainer::CraftingContainer(guard) => &mut **guard,
            LockedContainer::ResultContainer(guard) => &mut **guard,
            LockedContainer::Other(guard) => &mut **guard,
            LockedContainer::BlockEntity(guard) => (**guard)
                .as_container_mut()
                .expect("BlockEntity variant should only be used for container block entities"),
        }
    }
}

/// A reference to a container that can be locked.
///
/// This enum provides a uniform way to reference different container types
/// before locking them. Use [`ContainerLockGuard::lock_all`] to lock multiple
/// containers in a deadlock-free manner.
#[derive(Clone)]
pub enum ContainerRef {
    /// Reference to a player inventory.
    PlayerInventory(SyncPlayerInv),
    /// Reference to a crafting grid container.
    CraftingContainer(Arc<SyncMutex<CraftingContainer>>),
    /// Reference to a crafting result container.
    ResultContainer(Arc<SyncMutex<ResultContainer>>),
    /// Reference to a generic container.
    Other(GenericContainer),
    /// Reference to a block entity that implements Container.
    ///
    /// Use [`ContainerRef::from_block_entity`] to create this variant,
    /// which validates that the block entity actually implements Container.
    BlockEntity(SharedBlockEntity),
}

impl From<SyncPlayerInv> for ContainerRef {
    fn from(value: SyncPlayerInv) -> Self {
        Self::PlayerInventory(value)
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
        if is_container {
            Some(Self::BlockEntity(block_entity))
        } else {
            None
        }
    }

    /// Returns a unique identifier for this container based on its Arc pointer address.
    #[must_use]
    pub fn container_id(&self) -> ContainerId {
        match self {
            ContainerRef::PlayerInventory(arc) => ContainerId::from_arc(arc),
            ContainerRef::CraftingContainer(arc) => ContainerId::from_arc(arc),
            ContainerRef::ResultContainer(arc) => ContainerId::from_arc(arc),
            ContainerRef::Other(arc) => ContainerId::from_arc(arc),
            ContainerRef::BlockEntity(arc) => ContainerId::from_arc(arc),
        }
    }

    /// Locks this container and returns a guard.
    fn lock(&self) -> LockedContainer {
        match self {
            ContainerRef::PlayerInventory(arc) => {
                LockedContainer::PlayerInventory(SyncMutex::lock_arc(arc))
            }
            ContainerRef::CraftingContainer(arc) => {
                LockedContainer::CraftingContainer(SyncMutex::lock_arc(arc))
            }
            ContainerRef::ResultContainer(arc) => {
                LockedContainer::ResultContainer(SyncMutex::lock_arc(arc))
            }
            ContainerRef::Other(arc) => LockedContainer::Other(SyncMutex::lock_arc(arc)),
            ContainerRef::BlockEntity(arc) => {
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
/// let player_inv = ContainerRef::PlayerInventory(player_inv_arc);
/// let chest = ContainerRef::Other(chest_arc);
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

    /// Get mutable access to a locked player inventory.
    pub fn get_player_inventory_mut(
        &mut self,
        id: impl Into<ContainerId>,
    ) -> Option<&mut PlayerInventory> {
        self.id_to_index
            .get(&id.into())
            .copied()
            .and_then(|idx| self.guards.get_mut(idx))
            .and_then(|(_, guard)| match guard {
                LockedContainer::PlayerInventory(g) => Some(&mut **g),
                _ => None,
            })
    }

    /// Get immutable access to a locked crafting container.
    #[must_use]
    pub fn get_crafting_container(&self, id: impl Into<ContainerId>) -> Option<&CraftingContainer> {
        self.id_to_index
            .get(&id.into())
            .and_then(|&idx| self.guards.get(idx))
            .and_then(|(_, guard)| match guard {
                LockedContainer::CraftingContainer(g) => Some(&**g),
                _ => None,
            })
    }

    /// Get mutable access to a locked crafting container.
    pub fn get_crafting_container_mut(
        &mut self,
        id: impl Into<ContainerId>,
    ) -> Option<&mut CraftingContainer> {
        self.id_to_index
            .get(&id.into())
            .copied()
            .and_then(|idx| self.guards.get_mut(idx))
            .and_then(|(_, guard)| match guard {
                LockedContainer::CraftingContainer(g) => Some(&mut **g),
                _ => None,
            })
    }

    /// Get mutable access to a locked result container.
    pub fn get_result_container_mut(
        &mut self,
        id: impl Into<ContainerId>,
    ) -> Option<&mut ResultContainer> {
        self.id_to_index
            .get(&id.into())
            .copied()
            .and_then(|idx| self.guards.get_mut(idx))
            .and_then(|(_, guard)| match guard {
                LockedContainer::ResultContainer(g) => Some(&mut **g),
                _ => None,
            })
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
