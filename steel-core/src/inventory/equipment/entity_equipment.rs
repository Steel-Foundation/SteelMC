//! Entity equipment storage.

use std::mem;

use steel_registry::item_stack::ItemStack;

use super::EquipmentSlot;

/// Equipment storage for entities (armor, hands, etc.)
pub struct EntityEquipment {
    slots: [ItemStack; 8],
}

impl Default for EntityEquipment {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityEquipment {
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

    /// Gets a reference to the item in a slot.
    #[must_use]
    pub const fn get_ref(&self, slot: EquipmentSlot) -> &ItemStack {
        &self.slots[slot.index()]
    }

    /// Gets a mutable reference to the item in a slot.
    pub const fn get_mut(&mut self, slot: EquipmentSlot) -> &mut ItemStack {
        &mut self.slots[slot.index()]
    }

    /// Takes the item from a slot, leaving an empty stack in its place.
    pub fn take(&mut self, slot: EquipmentSlot) -> ItemStack {
        mem::take(&mut self.slots[slot.index()])
    }

    /// Sets the item in a slot, returning the old item.
    pub const fn set(&mut self, slot: EquipmentSlot, stack: ItemStack) -> ItemStack {
        mem::replace(&mut self.slots[slot.index()], stack)
    }

    /// Checks if a specific slot is empty.
    #[must_use]
    pub fn is_slot_empty(&self, slot: EquipmentSlot) -> bool {
        self.slots[slot.index()].is_empty()
    }

    /// Checks if all slots are empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(ItemStack::is_empty)
    }

    /// Clears all slots, replacing them with empty stacks.
    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            *slot = ItemStack::empty();
        }
    }
}
