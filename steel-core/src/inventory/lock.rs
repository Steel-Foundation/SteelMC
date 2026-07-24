//! Container locking utilities for deadlock-free multi-container operations.
//!
//! This module provides types for safely locking multiple containers in a
//! deterministic order to prevent deadlocks when performing operations that
//! span multiple inventories (e.g., transferring items between containers).

use parking_lot::ArcMutexGuard;
use parking_lot::RawMutex;
use rustc_hash::FxHashMap;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use steel_utils::{Downcast as _, DowncastType, locks::SyncMutex};

use crate::{
    block_entity::{BlockEntityBase, SharedBlockEntity},
    inventory::container::Container,
    player::{Player, player_inventory::PlayerInventory},
};
use steel_registry::item_stack::ItemStack;

/// Thread-safe reference to a player inventory.
pub type SyncPlayerInv = Arc<SyncMutex<PlayerInventory>>;

/// Thread-safe reference to an erased container.
pub type SharedContainer = Arc<SyncMutex<dyn Container>>;

struct LockedContainer(ArcMutexGuard<RawMutex, dyn Container>);

impl Deref for LockedContainer {
    type Target = dyn Container;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl DerefMut for LockedContainer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.0
    }
}

/// A reference to a container that can be locked.
///
/// The optional owner is notified only after all container locks have been
/// released at the corresponding Vanilla `setChanged` boundary.
#[derive(Clone)]
pub struct ContainerRef {
    id: ContainerId,
    source: SharedContainer,
    owner: Option<Arc<BlockEntityBase>>,
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
            source: container,
            owner: None,
        }
    }
}

impl From<SharedContainer> for ContainerRef {
    fn from(container: SharedContainer) -> Self {
        Self {
            id: ContainerId::from_arc(&container),
            source: container,
            owner: None,
        }
    }
}

impl ContainerRef {
    /// Creates a `ContainerRef` from a block entity, if it implements Container.
    ///
    /// Returns `None` if the block entity has no container capability.
    #[must_use]
    pub fn from_block_entity(block_entity: SharedBlockEntity) -> Option<Self> {
        block_entity.container_ref()
    }

    /// Creates a container capability owned by the block entity whose unique
    /// base is returned from its [`crate::block_entity::BlockEntity::base`].
    ///
    /// The container implementation must obey [`Container`]'s storage-local
    /// locking contract. This owner performs block-entity and comparator
    /// notifications after all container locks are released.
    #[must_use]
    pub fn owned_by_block_entity(container: SharedContainer, owner: Arc<BlockEntityBase>) -> Self {
        Self {
            id: ContainerId::from_arc(&container),
            source: container,
            owner: Some(owner),
        }
    }

    /// Returns a unique identifier for this container based on its Arc pointer address.
    #[must_use]
    pub const fn container_id(&self) -> ContainerId {
        self.id
    }

    /// Checks container access without locking its item storage.
    #[must_use]
    pub fn still_valid(&self, player: &Player) -> bool {
        self.owner
            .as_ref()
            .is_none_or(|owner| owner.is_valid_container_for(player))
    }

    /// Locks this container and returns a guard.
    fn lock(&self) -> LockedContainer {
        LockedContainer(SyncMutex::lock_arc(&self.source))
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
    // Stable sources retained while guards are temporarily released for an owner callback.
    sources: Vec<(ContainerId, ContainerRef)>,
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
        let mut sources: Vec<_> = containers
            .iter()
            .map(|container| (container.container_id(), (*container).clone()))
            .collect();

        // Sort by ID for deterministic lock order (prevents deadlocks)
        sources.sort_by_key(|(id, _)| *id);

        // Deduplicate (in case same container passed multiple times)
        sources.dedup_by_key(|(id, _)| *id);

        // Lock all in sorted order
        let mut guards = Vec::with_capacity(sources.len());
        for (id, container) in &sources {
            let guard = container.lock();
            guards.push((*id, guard));
        }

        // Build index map
        let id_to_index = guards
            .iter()
            .enumerate()
            .map(|(idx, (id, _))| (*id, idx))
            .collect();

        Self {
            sources,
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
    ///
    /// This bypasses owner notification and is only for deliberate no-update
    /// mutation or callers that establish the notification boundary separately.
    pub fn get_mut(&mut self, id: impl Into<ContainerId>) -> Option<&mut dyn Container> {
        self.id_to_index
            .get(&id.into())
            .copied()
            .and_then(|idx| self.guards.get_mut(idx))
            .map(|(_, guard)| &mut **guard as &mut dyn Container)
    }

    /// Mirrors a container's own `setItem` call.
    ///
    /// Vanilla block-entity containers call `BlockEntity::setChanged` from
    /// `setItem`, before a `Slot::set` performs its separate notification.
    pub fn set_item(&mut self, id: impl Into<ContainerId>, slot: usize, stack: ItemStack) -> bool {
        let id = id.into();
        let Some(&index) = self.id_to_index.get(&id) else {
            return false;
        };
        self.guards[index].1.set_item(slot, stack);
        let owner = self.sources[index].1.owner.clone();
        self.notify_owner(owner);
        true
    }

    /// Mirrors a container's own conditional `removeItem` notification.
    pub fn remove_item(
        &mut self,
        id: impl Into<ContainerId>,
        slot: usize,
        amount: i32,
    ) -> Option<ItemStack> {
        let id = id.into();
        let index = *self.id_to_index.get(&id)?;
        let removed = self.guards.get_mut(index)?.1.remove_item(slot, amount);
        if !removed.is_empty() {
            let owner = self.sources.get(index)?.1.owner.clone();
            self.notify_owner(owner);
        }
        Some(removed)
    }

    /// Calls `Container::set_changed` and synchronously notifies its owner
    /// after releasing every lock held by this guard.
    pub fn set_changed(&mut self, id: impl Into<ContainerId>) -> bool {
        let id = id.into();
        let Some(&index) = self.id_to_index.get(&id) else {
            return false;
        };
        self.guards[index].1.set_changed();
        let owner = self.sources[index].1.owner.clone();
        self.notify_owner(owner);
        true
    }

    /// Runs a callback after releasing every container, then reacquires the
    /// same sources in deterministic order before returning.
    pub(crate) fn run_unlocked<R>(&mut self, callback: impl FnOnce() -> R) -> R {
        drop(mem::take(&mut self.guards));
        let result = callback();
        self.guards = self
            .sources
            .iter()
            .map(|(id, container)| (*id, container.lock()))
            .collect();
        result
    }

    fn notify_owner(&mut self, owner: Option<Arc<BlockEntityBase>>) {
        let Some(owner) = owner else {
            return;
        };

        self.run_unlocked(|| owner.set_changed());
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
    ///
    /// This bypasses owner notification and is only for deliberate no-update
    /// mutation or callers that establish the notification boundary separately.
    pub fn get_typed_mut<T>(&mut self, id: impl Into<ContainerId>) -> Option<&mut T>
    where
        T: Container + DowncastType,
    {
        self.get_mut(id)?.downcast_mut::<T>()
    }

    /// Gets mutable access to two distinct concrete containers.
    ///
    /// This bypasses owner notification and is only for deliberate no-update
    /// mutation or callers that establish the notification boundary separately.
    pub fn get_two_typed_mut<A, B>(
        &mut self,
        first: impl Into<ContainerId>,
        second: impl Into<ContainerId>,
    ) -> Option<(&mut A, &mut B)>
    where
        A: Container + DowncastType,
        B: Container + DowncastType,
    {
        let first_index = *self.id_to_index.get(&first.into())?;
        let second_index = *self.id_to_index.get(&second.into())?;
        if first_index == second_index {
            return None;
        }

        let (first, second): (&mut dyn Container, &mut dyn Container) =
            if first_index < second_index {
                let (before_second, from_second) = self.guards.split_at_mut(second_index);
                (&mut *before_second[first_index].1, &mut *from_second[0].1)
            } else {
                let (before_first, from_first) = self.guards.split_at_mut(first_index);
                (&mut *from_first[0].1, &mut *before_first[second_index].1)
            };
        Some((first.downcast_mut::<A>()?, second.downcast_mut::<B>()?))
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

    use steel_registry::blocks::block_state_ext::BlockStateExt as _;
    use steel_registry::blocks::properties::{BlockStateProperties, Direction};
    use steel_registry::{
        item_stack::ItemStack, test_support::init_test_registry, vanilla_block_entity_types,
        vanilla_blocks, vanilla_items,
    };
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos, locks::SyncMutex};

    use super::{ContainerId, ContainerLockGuard, ContainerRef};
    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
    use crate::block_entity::{
        SharedBlockEntity,
        entities::{BarrelBlockEntity, RawBlockEntity},
        init_block_entities,
    };
    use crate::inventory::container::Container;
    use crate::inventory::crafting::{CraftingContainer, ResultContainer};
    use crate::inventory::slot::{NormalSlot, Slot as _};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

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
    fn block_entity_container_capability_is_independently_lockable() {
        init_test_registry();
        let barrel = Arc::new(BarrelBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::BARREL.default_state(),
        ));
        let block_entity: SharedBlockEntity = barrel.clone();
        let Some(container_ref) = ContainerRef::from_block_entity(block_entity) else {
            panic!("barrel block entity should expose Container");
        };
        let id = container_ref.container_id();

        let guard = ContainerLockGuard::lock_all(&[&container_ref]);
        assert_eq!(guard.get(id).map(Container::get_container_size), Some(27));
    }

    #[test]
    fn non_container_block_entity_ref_is_rejected() {
        init_test_registry();
        let block_entity: SharedBlockEntity = Arc::new(RawBlockEntity::new(
            &vanilla_block_entity_types::END_PORTAL,
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::END_PORTAL.default_state(),
        ));

        assert!(ContainerRef::from_block_entity(block_entity).is_none());
    }

    #[test]
    fn unlocked_callback_releases_and_reacquires_every_container() {
        let crafting = Arc::new(SyncMutex::new(CraftingContainer::new(2, 2)));
        let result = Arc::new(SyncMutex::new(ResultContainer::new()));
        let crafting_ref = ContainerRef::from(Arc::clone(&crafting));
        let result_ref = ContainerRef::from(Arc::clone(&result));
        let mut guard = ContainerLockGuard::lock_all(&[&crafting_ref, &result_ref]);

        let both_unlocked = guard.run_unlocked(|| {
            let crafting_guard = crafting.try_lock();
            let result_guard = result.try_lock();
            crafting_guard.is_some() && result_guard.is_some()
        });

        assert!(both_unlocked);
        assert!(guard.get(crafting_ref.container_id()).is_some());
        assert!(guard.get(result_ref.container_id()).is_some());
    }

    #[test]
    fn barrel_change_reenters_analog_read_without_holding_container_lock() {
        init_test_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("barrel_comparator_reentry");
        let barrel_pos = BlockPos::new(8, 64, 8);
        let comparator_pos = barrel_pos.west();
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(barrel_pos));
        assert!(world.set_block(
            comparator_pos.below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            barrel_pos,
            vanilla_blocks::BARREL.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let comparator_state = vanilla_blocks::COMPARATOR
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::East);
        assert!(world.set_block(comparator_pos, comparator_state, UpdateFlags::UPDATE_NONE,));
        assert!(!world.has_scheduled_block_tick(comparator_pos, &vanilla_blocks::COMPARATOR));

        let container_ref = ContainerRef::from_block_entity(
            world
                .get_block_entity(barrel_pos)
                .expect("barrel should create its block entity"),
        )
        .expect("barrel should expose a container capability");
        let slot = NormalSlot::new(container_ref.clone(), 0);
        let mut guard = ContainerLockGuard::lock_all(&[&container_ref]);
        slot.set_item(&mut guard, ItemStack::new(&vanilla_items::STONE));
        drop(guard);

        let analog = BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::BARREL)
            .get_analog_output_signal(
                world.get_block_state(barrel_pos),
                world.as_ref(),
                barrel_pos,
                Direction::West,
            );
        assert_eq!(analog, 1);
        assert!(
            world.has_scheduled_block_tick(comparator_pos, &vanilla_blocks::COMPARATOR),
            "barrel callback did not schedule the comparator despite analog output {analog}"
        );
    }
}
