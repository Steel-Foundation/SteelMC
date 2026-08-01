//! Slot abstraction for inventory access.

use steel_registry::item_stack::ItemStack;
use steel_utils::ErasedType;

use crate::inventory::lock::{ContainerId, ContainerLockGuard, ContainerRef};
use crate::player::Player;

/// Physical storage and auxiliary container dependencies used by a [`Slot`].
///
/// Menu builders derive their complete lock set and physical-alias checks from
/// this descriptor, so custom slots must include every container accessed by
/// any slot callback.
pub struct SlotStorage {
    physical: Option<(ContainerRef, usize)>,
    dependencies: Vec<ContainerRef>,
}

impl SlotStorage {
    /// Describes a slot backed by one physical container position.
    #[must_use]
    pub fn physical(container: impl Into<ContainerRef>, index: usize) -> Self {
        Self {
            physical: Some((container.into(), index)),
            dependencies: Vec::new(),
        }
    }

    /// Describes a storage-less slot that only accesses auxiliary containers.
    #[must_use]
    pub fn virtual_slot(dependencies: impl IntoIterator<Item = impl Into<ContainerRef>>) -> Self {
        Self {
            physical: None,
            dependencies: dependencies.into_iter().map(Into::into).collect(),
        }
    }

    /// Adds auxiliary containers accessed by slot callbacks.
    #[must_use]
    pub fn with_dependencies(
        mut self,
        dependencies: impl IntoIterator<Item = impl Into<ContainerRef>>,
    ) -> Self {
        self.dependencies
            .extend(dependencies.into_iter().map(Into::into));
        self
    }

    /// Returns the slot's physical container and container-local index.
    #[must_use]
    pub fn physical_backing(&self) -> Option<(&ContainerRef, usize)> {
        self.physical
            .as_ref()
            .map(|(container, index)| (container, *index))
    }

    /// Returns the stable identity of the physical backing position.
    #[must_use]
    pub fn physical_key(&self) -> Option<(ContainerId, usize)> {
        self.physical_backing()
            .map(|(container, index)| (container.container_id(), index))
    }

    pub(crate) fn container_refs(&self) -> impl Iterator<Item = &ContainerRef> {
        self.physical
            .iter()
            .map(|(container, _)| container)
            .chain(self.dependencies.iter())
    }
}

/// A view into a single position in a container, accessed via a `ContainerLockGuard`.
///
/// Concrete implementations must implement [`steel_utils::DowncastType`] with
/// a unique, stable key so erased slot references can recover their type, and
/// retain one [`SlotStorage`] descriptor for their full container dependency
/// set.
pub trait Slot: ErasedType + Send + Sync {
    /// Returns this slot's physical storage and auxiliary dependencies.
    fn storage(&self) -> &SlotStorage;

    /// Returns a reference to the item in this slot.
    fn get_item<'a>(&self, guard: &'a ContainerLockGuard) -> &'a ItemStack;

    /// Returns a mutable reference to the item in this slot.
    fn get_item_mut<'a>(&self, guard: &'a mut ContainerLockGuard) -> &'a mut ItemStack;

    /// Sets the item in this slot.
    fn set_item(&self, guard: &mut ContainerLockGuard, stack: ItemStack);

    /// Sets the item, triggered by a player action. `previous` is the prior item.
    fn set_by_player(
        &self,
        guard: &mut ContainerLockGuard,
        stack: ItemStack,
        _previous: &ItemStack,
    ) {
        self.set_item(guard, stack);
    }

    /// Returns true if this slot has an item.
    fn has_item(&self, guard: &ContainerLockGuard) -> bool {
        !self.get_item(guard).is_empty()
    }

    /// Returns true if the given item can be placed in this slot.
    fn may_place(&self, _stack: &ItemStack) -> bool {
        true
    }

    /// Returns true if items can be picked up from this slot.
    fn may_pickup(&self, _guard: &ContainerLockGuard, _player: &Player) -> bool {
        true
    }

    /// Returns true if partial removal is allowed from this slot.
    fn allow_modification(&self, guard: &ContainerLockGuard, player: &Player) -> bool {
        self.may_pickup(guard, player) && self.may_place(self.get_item(guard))
    }

    /// Returns the maximum stack size for this slot.
    fn get_max_stack_size(&self, guard: &ContainerLockGuard) -> i32;

    /// Returns the max stack size for `stack` here (min of slot and item limits).
    fn get_max_stack_size_for_item(&self, guard: &ContainerLockGuard, stack: &ItemStack) -> i32 {
        self.get_max_stack_size(guard).min(stack.max_stack_size())
    }

    /// Removes up to `amount` items from this slot and returns them.
    fn remove(&self, guard: &mut ContainerLockGuard, amount: i32) -> ItemStack {
        let item = self.get_item_mut(guard);
        if item.is_empty() || amount <= 0 {
            return ItemStack::empty();
        }
        item.split(amount)
    }

    /// Tries to remove items from this slot with validation.
    fn try_remove(
        &self,
        guard: &mut ContainerLockGuard,
        amount: i32,
        max_amount: i32,
        player: &Player,
    ) -> Option<ItemStack> {
        if !self.may_pickup(guard, player) {
            return None;
        }

        let item_count = self.get_item(guard).count();

        if !self.allow_modification(guard, player) && max_amount < item_count {
            return None;
        }

        let take_amount = amount.min(max_amount);
        let result = self.remove(guard, take_amount);
        if result.is_empty() {
            return None;
        }

        if self.get_item(guard).is_empty() {
            self.set_by_player(guard, ItemStack::empty(), &result);
        }

        Some(result)
    }

    /// Called when an item is taken. Returns any remainder that couldn't be placed back.
    fn on_take(
        &self,
        guard: &mut ContainerLockGuard,
        _stack: &ItemStack,
        _player: &Player,
    ) -> Option<ItemStack> {
        self.set_changed(guard);
        None
    }

    /// Takes items with all checks and callbacks. Returns the items taken.
    fn safe_take(
        &self,
        guard: &mut ContainerLockGuard,
        amount: i32,
        max_amount: i32,
        player: &Player,
    ) -> ItemStack {
        if let Some(taken) = self.try_remove(guard, amount, max_amount, player) {
            if let Some(remainder) = self.on_take(guard, &taken, player) {
                player.add_item_or_drop_with_guard(guard, remainder);
            }
            taken
        } else {
            ItemStack::empty()
        }
    }

    /// Inserts up to `amount` items, firing set callbacks.
    fn safe_insert(
        &self,
        guard: &mut ContainerLockGuard,
        mut input: ItemStack,
        amount: i32,
    ) -> ItemStack {
        if input.is_empty() || !self.may_place(&input) {
            return input;
        }

        let slot_stack = self.get_item(guard).clone();
        let transferable = amount
            .min(input.count)
            .min(self.get_max_stack_size_for_item(guard, &input) - slot_stack.count);
        if transferable <= 0 {
            return input;
        }

        if slot_stack.is_empty() {
            self.set_by_player(guard, input.split(transferable), &slot_stack);
        } else if ItemStack::is_same_item_same_components(&slot_stack, &input) {
            input.shrink(transferable);
            let mut new_slot_stack = slot_stack.clone();
            new_slot_stack.grow(transferable);
            self.set_by_player(guard, new_slot_stack, &slot_stack);
        }

        input
    }

    /// Marks the slot's container as changed.
    fn set_changed(&self, guard: &mut ContainerLockGuard);

    /// Returns the container slot index.
    fn get_container_slot(&self) -> usize;

    /// Returns true if normal menu persistence and transfer rules must not be
    /// applied to this slot.
    ///
    /// A container-backed fake slot must be the menu's only view of its
    /// physical [`SlotStorage::physical_key`].
    fn is_fake(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use steel_registry::{test_support::init_test_registry, vanilla_items};
    use steel_utils::locks::IntoShared;
    use steel_utils::{Downcast as _, DowncastType, DowncastTypeKey};

    use super::*;
    use crate::inventory::slots::normal_slot::NormalSlot;
    use crate::inventory::{container::SimpleContainer, lock::ContainerRef};

    struct SafeInsertOverrideSlot {
        base: NormalSlot,
        called: Arc<AtomicBool>,
    }

    // SAFETY: This test-only key uniquely identifies `SafeInsertOverrideSlot`.
    unsafe impl DowncastType for SafeInsertOverrideSlot {
        const TYPE_KEY: DowncastTypeKey =
            DowncastTypeKey::new("steel:test/slot/safe_insert_override");
    }

    impl Slot for SafeInsertOverrideSlot {
        fn storage(&self) -> &SlotStorage {
            self.base.storage()
        }

        fn get_item<'a>(&self, guard: &'a ContainerLockGuard) -> &'a ItemStack {
            self.base.get_item(guard)
        }

        fn get_item_mut<'a>(&self, guard: &'a mut ContainerLockGuard) -> &'a mut ItemStack {
            self.base.get_item_mut(guard)
        }

        fn set_item(&self, guard: &mut ContainerLockGuard, stack: ItemStack) {
            self.base.set_item(guard, stack);
        }

        fn safe_insert(
            &self,
            _guard: &mut ContainerLockGuard,
            input: ItemStack,
            _amount: i32,
        ) -> ItemStack {
            self.called.store(true, Ordering::Relaxed);
            input
        }

        fn get_max_stack_size(&self, guard: &ContainerLockGuard) -> i32 {
            self.base.get_max_stack_size(guard)
        }

        fn set_changed(&self, guard: &mut ContainerLockGuard) {
            self.base.set_changed(guard);
        }

        fn get_container_slot(&self) -> usize {
            self.base.get_container_slot()
        }
    }

    #[test]
    fn custom_slot_safe_insert_override_survives_erasure() {
        init_test_registry();
        let container = SimpleContainer::new(1).into_shared();
        let container_ref = ContainerRef::from(Arc::clone(&container));
        let called = Arc::new(AtomicBool::new(false));
        let slot: Box<dyn Slot> = Box::new(SafeInsertOverrideSlot {
            base: NormalSlot::new(container_ref.clone(), 0),
            called: Arc::clone(&called),
        });
        assert!(slot.downcast_ref::<SafeInsertOverrideSlot>().is_some());
        let mut guard = ContainerLockGuard::lock_all(&[container_ref]);

        let remaining = slot.safe_insert(&mut guard, ItemStack::new(&vanilla_items::STONE), 1);

        assert!(called.load(Ordering::Relaxed));
        assert!(remaining.is(&vanilla_items::STONE));
        assert!(slot.get_item(&guard).is_empty());
    }
}
