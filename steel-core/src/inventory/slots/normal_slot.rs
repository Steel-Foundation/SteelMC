use steel_registry::item_stack::ItemStack;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::inventory::{
    lock::{ContainerLockGuard, ContainerRef},
    slots::slot::{Slot, SlotStorage},
};

/// A normal slot that references a container and index.
pub struct NormalSlot {
    storage: SlotStorage,
}

// SAFETY: This key uniquely identifies Steel's `NormalSlot`.
unsafe impl DowncastType for NormalSlot {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:slot/normal");
}

impl NormalSlot {
    /// Creates a new normal slot from a `ContainerRef`.
    pub fn new(container: impl Into<ContainerRef>, index: usize) -> Self {
        Self {
            storage: SlotStorage::physical(container, index),
        }
    }

    /// Returns a reference to the container.
    #[must_use]
    pub fn container_ref(&self) -> ContainerRef {
        self.backing().0.clone()
    }

    fn backing(&self) -> (&ContainerRef, usize) {
        let Some(backing) = self.storage.physical_backing() else {
            unreachable!("NormalSlot always has physical storage");
        };
        backing
    }
}

impl Slot for NormalSlot {
    fn storage(&self) -> &SlotStorage {
        &self.storage
    }

    fn get_item<'a>(&self, guard: &'a ContainerLockGuard) -> &'a ItemStack {
        let (container, index) = self.backing();
        guard
            .get(container.container_id())
            .expect("container not locked")
            .get_item(index)
    }

    fn get_item_mut<'a>(&self, guard: &'a mut ContainerLockGuard) -> &'a mut ItemStack {
        let (container, index) = self.backing();
        guard
            .get_mut(container.container_id())
            .expect("container not locked")
            .get_item_mut(index)
    }

    fn set_item(&self, guard: &mut ContainerLockGuard, stack: ItemStack) {
        let (container, index) = self.backing();
        assert!(
            guard.set_item(container.container_id(), index, stack),
            "container not locked"
        );
        self.set_changed(guard);
    }

    fn remove(&self, guard: &mut ContainerLockGuard, amount: i32) -> ItemStack {
        if amount <= 0 || self.get_item(guard).is_empty() {
            return ItemStack::empty();
        }
        let (container, index) = self.backing();
        guard
            .remove_item(container.container_id(), index, amount)
            .expect("container not locked")
    }

    fn set_changed(&self, guard: &mut ContainerLockGuard) {
        let (container, _) = self.backing();
        assert!(
            guard.set_changed(container.container_id()),
            "container not locked"
        );
    }

    fn get_container_slot(&self) -> usize {
        self.backing().1
    }

    fn get_max_stack_size(&self, guard: &ContainerLockGuard) -> i32 {
        let (container, _) = self.backing();
        guard
            .get(container.container_id())
            .expect("container not locked")
            .get_max_stack_size()
    }
}
