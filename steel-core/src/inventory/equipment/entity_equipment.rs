//! Entity equipment storage.

use std::mem;

use steel_registry::item_stack::ItemStack;

use super::EquipmentSlot;

/// Equipment storage for entities (armor, hands, etc.)
pub struct EntityEquipment {
    slots: [ItemStack; 8],
    dirty_slots: [bool; 8],
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
            dirty_slots: [false; 8],
        }
    }

    /// Gets a reference to the item in a slot.
    #[must_use]
    pub const fn get_ref(&self, slot: EquipmentSlot) -> &ItemStack {
        &self.slots[slot.index()]
    }

    /// Gets a mutable reference to the item in a slot.
    pub const fn get_mut(&mut self, slot: EquipmentSlot) -> &mut ItemStack {
        self.mark_dirty(slot);
        &mut self.slots[slot.index()]
    }

    /// Takes the item from a slot, leaving an empty stack in its place.
    pub fn take(&mut self, slot: EquipmentSlot) -> ItemStack {
        let old = mem::take(&mut self.slots[slot.index()]);
        if !old.is_empty() {
            self.mark_dirty(slot);
        }
        old
    }

    /// Sets the item in a slot, returning the old item.
    pub fn set(&mut self, slot: EquipmentSlot, stack: ItemStack) -> ItemStack {
        let old = mem::replace(&mut self.slots[slot.index()], stack);
        if old != self.slots[slot.index()] {
            self.mark_dirty(slot);
        }
        old
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
        for slot in EquipmentSlot::ALL {
            if !self.slots[slot.index()].is_empty() {
                self.slots[slot.index()] = ItemStack::empty();
                self.mark_dirty(slot);
            }
        }
    }

    /// Returns non-empty equipment slots for initial spawn synchronization.
    #[must_use]
    pub fn non_empty_items(&self) -> Vec<(EquipmentSlot, ItemStack)> {
        EquipmentSlot::ALL
            .into_iter()
            .filter_map(|slot| {
                let item = self.get_ref(slot);
                (!item.is_empty()).then(|| (slot, item.clone()))
            })
            .collect()
    }

    /// Drains equipment slots that changed since the last sync.
    pub fn drain_dirty_items(&mut self) -> Vec<(EquipmentSlot, ItemStack)> {
        let mut dirty_items = Vec::new();
        for slot in EquipmentSlot::ALL {
            let index = slot.index();
            if !self.dirty_slots[index] {
                continue;
            }
            self.dirty_slots[index] = false;
            dirty_items.push((slot, self.slots[index].clone()));
        }
        dirty_items
    }

    const fn mark_dirty(&mut self, slot: EquipmentSlot) {
        self.dirty_slots[slot.index()] = true;
    }
}
