//! Entity equipment access and owned storage.

use std::mem;

use steel_registry::item_stack::ItemStack;

pub use steel_registry::equipment::{EquipmentSlot, EquipmentSlotType};

/// Equipment access shared by player inventories and owned entity storage.
pub trait EntityEquipment: Send {
    /// Gets a reference to the item in a slot.
    fn get_ref(&self, slot: EquipmentSlot) -> &ItemStack;

    /// Gets a mutable reference to the item in a slot.
    fn get_mut(&mut self, slot: EquipmentSlot) -> &mut ItemStack;

    /// Sets the item in a slot, returning the old item.
    fn set(&mut self, slot: EquipmentSlot, stack: ItemStack) -> ItemStack;

    /// Takes the item from a slot, leaving an empty stack in its place.
    fn take(&mut self, slot: EquipmentSlot) -> ItemStack;

    /// Clears all equipment slots.
    fn clear(&mut self);

    /// Returns non-empty equipment slots for initial spawn synchronization.
    fn non_empty_items(&self) -> Vec<(EquipmentSlot, ItemStack)> {
        EquipmentSlot::ALL
            .into_iter()
            .filter_map(|slot| {
                let item = self.get_ref(slot);
                (!item.is_empty()).then(|| (slot, item.clone()))
            })
            .collect()
    }
}

/// Owned equipment storage used by non-player living entities.
pub struct OwnedEntityEquipment {
    slots: [ItemStack; 8],
}

impl Default for OwnedEntityEquipment {
    fn default() -> Self {
        Self::new()
    }
}

impl OwnedEntityEquipment {
    /// Creates a new empty equipment storage.
    #[must_use]
    pub fn new() -> Self {
        Self {
            slots: [
                ItemStack::empty(),
                ItemStack::empty(),
                ItemStack::empty(),
                ItemStack::empty(),
                ItemStack::empty(),
                ItemStack::empty(),
                ItemStack::empty(),
                ItemStack::empty(),
            ],
        }
    }
}

impl EntityEquipment for OwnedEntityEquipment {
    fn get_ref(&self, slot: EquipmentSlot) -> &ItemStack {
        &self.slots[slot.index()]
    }

    fn get_mut(&mut self, slot: EquipmentSlot) -> &mut ItemStack {
        &mut self.slots[slot.index()]
    }

    fn set(&mut self, slot: EquipmentSlot, stack: ItemStack) -> ItemStack {
        mem::replace(&mut self.slots[slot.index()], stack)
    }

    fn take(&mut self, slot: EquipmentSlot) -> ItemStack {
        mem::take(&mut self.slots[slot.index()])
    }

    fn clear(&mut self) {
        for slot in EquipmentSlot::ALL {
            self.slots[slot.index()] = ItemStack::empty();
        }
    }
}
