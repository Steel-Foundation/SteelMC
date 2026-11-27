//! Slots specific to the enchanting-table menu.

use steel_registry::item_stack::ItemStack;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::slots::{NormalSlot, Slot, SlotStorage};

/// The enchanting table item slot accepts anything but holds a single item
pub struct EnchantItemSlot {
    base: NormalSlot,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EnchantItemSlot`.
unsafe impl DowncastType for EnchantItemSlot {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:slot/enchant_item");
}

impl EnchantItemSlot {
    /// Creates the slot over `index` of `container`.
    pub fn new(container: impl Into<ContainerRef>, index: usize) -> Self {
        Self {
            base: NormalSlot::new(container, index),
        }
    }
}

impl Slot for EnchantItemSlot {
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

    fn get_max_stack_size(&self, _guard: &ContainerLockGuard) -> i32 {
        1
    }

    fn set_changed(&self, guard: &mut ContainerLockGuard) {
        self.base.set_changed(guard);
    }

    fn get_container_slot(&self) -> usize {
        self.base.get_container_slot()
    }
}
