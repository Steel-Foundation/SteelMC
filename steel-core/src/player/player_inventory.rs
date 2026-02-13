//! Player inventory management.

use std::{
    array,
    sync::{LazyLock, Weak},
};

use steel_registry::item_stack::ItemStack;
use steel_utils::types::InteractionHand;

use crate::{
    inventory::{
        container::Container,
        equipment::{EntityEquipment, EquipmentSlot},
    },
    player::Player,
};

/// Maps inventory slot indices (36+) to equipment slots.
/// Slots 36-39: Armor (feet, legs, chest, head)
/// Slot 40: Offhand
/// Slot 41: Body armor (for animals, not used for players)
/// Slot 42: Saddle (for animals, not used for players)
const fn slot_to_equipment(slot: usize) -> Option<EquipmentSlot> {
    match slot {
        36 => Some(EquipmentSlot::Feet),
        37 => Some(EquipmentSlot::Legs),
        38 => Some(EquipmentSlot::Chest),
        39 => Some(EquipmentSlot::Head),
        40 => Some(EquipmentSlot::OffHand),
        41 => Some(EquipmentSlot::Body),
        42 => Some(EquipmentSlot::Saddle),
        _ => None,
    }
}

/// Player inventory container managing the main inventory and equipment.
///
/// Contains 36 main inventory slots (0-8 hotbar, 9-35 main) plus equipment slots
/// (armor, offhand, etc.) accessed through the Container trait.
pub struct PlayerInventory {
    /// The 36 main inventory slots (0-8 hotbar, 9-35 main).
    items: [ItemStack; Self::INVENTORY_SIZE],
    /// Entity equipment (armor, hands).
    equipment: EntityEquipment,
    /// Weak reference to the player.
    #[allow(dead_code)]
    player: Weak<Player>,
    /// Currently selected hotbar slot (0-8).
    selected: u8,
    /// Counter incremented on every change.
    times_changed: u32,
}

impl PlayerInventory {
    /// Number of main inventory slots.
    pub const INVENTORY_SIZE: usize = 36;
    /// Number of hotbar slots.
    pub const SELECTION_SIZE: usize = 9;
    /// Slot index for offhand.
    pub const SLOT_OFFHAND: usize = 40;

    /// Creates a new player inventory with empty slots.
    #[must_use]
    pub fn new(player: Weak<Player>) -> Self {
        Self {
            items: array::from_fn(|_| ItemStack::empty()),
            equipment: EntityEquipment::new(),
            player,
            selected: 0,
            times_changed: 0,
        }
    }

    /// Returns a reference to the entity equipment.
    #[must_use]
    pub const fn equipment(&self) -> &EntityEquipment {
        &self.equipment
    }

    /// Returns a mutable reference to the entity equipment.
    pub const fn equipment_mut(&mut self) -> &mut EntityEquipment {
        &mut self.equipment
    }

    /// Returns true if the given slot index is a hotbar slot (0-8).
    #[must_use]
    pub const fn is_hotbar_slot(slot: usize) -> bool {
        slot < Self::SELECTION_SIZE
    }

    /// Returns the currently selected hotbar slot (0-8).
    #[must_use]
    pub const fn get_selected_slot(&self) -> u8 {
        self.selected
    }

    /// Sets the selected hotbar slot.
    ///
    /// # Panics
    ///
    /// Panics if the slot is not a valid hotbar slot (must be 0-8).
    pub fn set_selected_slot(&mut self, slot: u8) {
        if Self::is_hotbar_slot(slot as usize) {
            self.selected = slot;
        } else {
            panic!("Invalid hotbar slot: {slot}");
        }
    }

    /// Executes a function with a reference to the currently selected item.
    pub fn with_selected_item<R>(&self, f: impl FnOnce(&ItemStack) -> R) -> R {
        f(&self.items[self.selected as usize])
    }

    /// Returns a clone of the currently selected item (main hand).
    #[must_use]
    pub const fn get_selected_item(&self) -> &ItemStack {
        &self.items[self.selected as usize]
    }

    /// Returns a clone of the currently selected item (main hand).
    #[must_use]
    pub const fn get_selected_item_mut(&mut self) -> &mut ItemStack {
        &mut self.items[self.selected as usize]
    }

    /// Sets the currently selected item (main hand).
    pub fn set_selected_item(&mut self, item: ItemStack) {
        self.items[self.selected as usize] = item;
        self.set_changed();
    }

    /// Returns a clone of the offhand item.
    #[must_use]
    pub const fn get_offhand_item(&self) -> &ItemStack {
        self.equipment.get_ref(EquipmentSlot::OffHand)
    }

    /// Returns a clone of the offhand item.
    #[must_use]
    pub const fn get_offhand_item_mut(&mut self) -> &mut ItemStack {
        self.equipment.get_mut(EquipmentSlot::OffHand)
    }

    /// Sets the offhand item.
    pub fn set_offhand_item(&mut self, item: ItemStack) {
        self.equipment.set(EquipmentSlot::OffHand, item);
        self.set_changed();
    }

    /// Executes a function with a mutable reference to the currently selected item.
    pub fn with_selected_item_mut<R>(&mut self, f: impl FnOnce(&mut ItemStack) -> R) -> R {
        let result = f(&mut self.items[self.selected as usize]);
        self.set_changed();
        result
    }

    /// Returns the number of times this inventory has been modified.
    #[must_use]
    pub const fn get_times_changed(&self) -> u32 {
        self.times_changed
    }

    /// Returns the non-equipment items (main 36 slots).
    #[must_use]
    pub const fn get_items(&self) -> &[ItemStack; Self::INVENTORY_SIZE] {
        &self.items
    }

    /// Finds the first empty slot in the inventory, or -1 if full.
    #[must_use]
    pub fn get_free_slot(&self) -> i32 {
        for i in 0..self.items.len() {
            if self.items[i].is_empty() {
                return i as i32;
            }
        }
        -1
    }

    /// Finds a slot containing an item matching the given stack (same item type).
    /// Returns -1 if not found.
    #[must_use]
    pub fn find_slot_matching_item(&self, stack: &ItemStack) -> i32 {
        for i in 0..self.items.len() {
            if !self.items[i].is_empty() && ItemStack::is_same_item(&self.items[i], stack) {
                return i as i32;
            }
        }
        -1
    }

    /// Swaps items between selected hotbar slot and the given slot.
    /// Used for pick block when item is in main inventory but not hotbar.
    pub fn pick_slot(&mut self, slot: i32) {
        let slot = slot as usize;
        if slot >= self.items.len() {
            return;
        }
        let selected = self.selected as usize;
        self.items.swap(selected, slot);
        self.set_changed();
    }

    /// Adds an item to the hotbar (for creative pick block) and selects it.
    /// Returns true if successful.
    pub fn add_and_pick_item(&mut self, stack: ItemStack) -> bool {
        // Find first empty hotbar slot
        for i in 0..Self::SELECTION_SIZE {
            if self.items[i].is_empty() {
                self.items[i] = stack;
                self.selected = i as u8;
                self.set_changed();
                return true;
            }
        }
        // No empty slot, replace current slot
        self.items[self.selected as usize] = stack;
        self.set_changed();
        true
    }

    /// Gets the item in the specified hand.
    #[must_use]
    pub const fn get_item_in_hand(&self, hand: InteractionHand) -> &ItemStack {
        match hand {
            InteractionHand::MainHand => self.get_selected_item(),
            InteractionHand::OffHand => self.get_offhand_item(),
        }
    }

    /// Gets the item in the specified hand.
    #[must_use]
    pub const fn get_item_in_hand_mut(&mut self, hand: InteractionHand) -> &mut ItemStack {
        match hand {
            InteractionHand::MainHand => self.get_selected_item_mut(),
            InteractionHand::OffHand => self.get_offhand_item_mut(),
        }
    }

    /// Sets the item in the specified hand.
    pub fn set_item_in_hand(&mut self, hand: InteractionHand, item: ItemStack) {
        match hand {
            InteractionHand::MainHand => self.set_selected_item(item),
            InteractionHand::OffHand => self.set_offhand_item(item),
        }
    }
}

/// Static empty item stack for returning references to invalid slots.
static EMPTY_ITEM: LazyLock<ItemStack> = LazyLock::new(ItemStack::empty);

impl Container for PlayerInventory {
    fn get_container_size(&self) -> usize {
        // 36 main slots + 7 equipment slots (feet, legs, chest, head, offhand, body, saddle)
        Self::INVENTORY_SIZE + 7
    }

    /// Adds an item to the player's main inventory (slots 0-35 only).
    ///
    /// Overrides the default `Container::add()` to prevent items from being
    /// placed in armor or equipment slots. Matches vanilla's `Inventory.add()`
    /// behavior which only adds to `this.items` (the 36 main slots).
    fn add(&mut self, stack: &mut ItemStack) -> bool {
        if stack.is_empty() {
            return true;
        }

        let max_size = self.get_max_stack_size_for_item(stack);

        // First pass: try to stack with existing items in main inventory only
        if stack.is_stackable() {
            for slot in 0..Self::INVENTORY_SIZE {
                if stack.is_empty() {
                    return true;
                }
                let existing = &mut self.items[slot];
                if !existing.is_empty() && ItemStack::is_same_item_same_components(existing, stack)
                {
                    let space = max_size - existing.count();
                    if space > 0 {
                        let to_add = stack.count().min(space);
                        existing.grow(to_add);
                        stack.shrink(to_add);
                    }
                }
            }
        }

        // Second pass: try empty slots in main inventory only
        for slot in 0..Self::INVENTORY_SIZE {
            if stack.is_empty() {
                return true;
            }
            if self.items[slot].is_empty() {
                let to_place = stack.count().min(max_size);
                let mut placed = stack.clone();
                placed.set_count(to_place);
                self.items[slot] = placed;
                stack.shrink(to_place);
            }
        }

        self.set_changed();
        stack.is_empty()
    }

    fn get_item(&self, slot: usize) -> &ItemStack {
        if slot < Self::INVENTORY_SIZE {
            &self.items[slot]
        } else if let Some(eq_slot) = slot_to_equipment(slot) {
            self.equipment.get_ref(eq_slot)
        } else {
            &EMPTY_ITEM
        }
    }

    fn get_item_mut(&mut self, slot: usize) -> &mut ItemStack {
        if slot < Self::INVENTORY_SIZE {
            &mut self.items[slot]
        } else if let Some(eq_slot) = slot_to_equipment(slot) {
            self.equipment.get_mut(eq_slot)
        } else {
            // Invalid slot - this is a bug, but we need to return something.
            // Return the first item slot as a fallback (will be logged in debug builds).
            debug_assert!(false, "Invalid slot index: {slot}");
            &mut self.items[0]
        }
    }

    fn set_item(&mut self, slot: usize, stack: ItemStack) {
        if slot < Self::INVENTORY_SIZE {
            self.items[slot] = stack;
        } else if let Some(eq_slot) = slot_to_equipment(slot) {
            self.equipment.set(eq_slot, stack);
        }
        self.set_changed();
    }

    fn is_empty(&self) -> bool {
        for item in &self.items {
            if !item.is_empty() {
                return false;
            }
        }

        for slot in EquipmentSlot::ALL {
            if !self.equipment.get_ref(slot).is_empty() {
                return false;
            }
        }

        true
    }

    fn set_changed(&mut self) {
        self.times_changed = self.times_changed.wrapping_add(1);
    }

    fn clear_content(&mut self) -> i32 {
        let mut count = 0;
        for item in &mut self.items {
            count += item.count;
            *item = ItemStack::empty();
        }
        self.equipment.clear();
        self.set_changed();
        count
    }
}
