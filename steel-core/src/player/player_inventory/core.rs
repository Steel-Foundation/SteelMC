use std::{array, ops::Range};

use simdnbt::owned::{NbtList, NbtTag};
use steel_registry::{enchantment_effect::EnchantmentEffectComponent, item_stack::ItemStack};
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::inventory::{
    container::Container,
    equipment::{EntityEquipment, EquipmentSlot},
};

use super::container::InvalidHotbarSlot;

/// Player inventory container managing the main inventory and equipment.
///
/// Contains 36 main inventory slots (0-8 hotbar, 9-35 main) plus equipment slots
/// (armor, offhand, etc.) accessed through the Container trait.
pub struct PlayerInventory {
    /// All 43 logical inventory slots in vanilla container order.
    pub(super) items: [ItemStack; Self::CONTAINER_SIZE],
    /// Currently selected hotbar slot (0-8).
    pub(super) selected: u8,
    /// Counter incremented on every change.
    pub(super) times_changed: u32,
}

impl Default for PlayerInventory {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: This key is owned by Steel and uniquely identifies `PlayerInventory`.
unsafe impl DowncastType for PlayerInventory {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/player_inventory");
}

impl PlayerInventory {
    pub(crate) fn take_death_drops(&mut self) -> Vec<ItemStack> {
        (0..self.get_container_size())
            .filter_map(|slot| {
                let item = self.get_item(slot).clone();
                if item.is_empty() {
                    return None;
                }
                self.set_item(slot, ItemStack::empty());
                (!item.has_enchantment_effect(EnchantmentEffectComponent::PreventEquipmentDrop))
                    .then_some(item)
            })
            .collect()
    }

    /// Number of main inventory slots.
    pub const INVENTORY_SIZE: usize = 36;
    /// Number of logical container slots, including equipment.
    pub const CONTAINER_SIZE: usize = 43;
    /// Number of hotbar slots.
    pub const SELECTION_SIZE: usize = 9;
    /// Slot index for offhand.
    pub const SLOT_OFFHAND: usize = 40;
    /// Hotbar container indices.
    pub const HOTBAR: Range<usize> = 0..9;
    /// Main storage container indices (everything except hotbar, armor, offhand).
    pub const MAIN: Range<usize> = 9..36;
    /// Armor container indices in display order (head, chest, legs, feet).
    pub const ARMOR_TOP_DOWN: [usize; 4] = [39, 38, 37, 36];

    /// Creates a new player inventory with empty slots.
    #[must_use]
    pub fn new() -> Self {
        Self {
            items: array::from_fn(|_| ItemStack::empty()),
            selected: 0,
            times_changed: 0,
        }
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

    /// Serializes the main inventory with vanilla's `ItemStackWithSlot` shape.
    #[must_use]
    pub(crate) fn to_vanilla_inventory_nbt(&self) -> NbtList {
        let items = self.items[..Self::INVENTORY_SIZE]
            .iter()
            .enumerate()
            .filter_map(|(slot, item)| {
                if item.is_empty() {
                    return None;
                }
                let NbtTag::Compound(mut nbt) = item.to_nbt_tag_ref() else {
                    return None;
                };
                nbt.insert("Slot", NbtTag::Byte(slot as i8));
                Some(nbt)
            })
            .collect();
        NbtList::Compound(items)
    }

    /// Sets the selected hotbar slot.
    ///
    /// # Panics
    ///
    /// Panics if the slot is not a valid hotbar slot (must be 0-8).
    pub fn set_selected_slot(&mut self, slot: u8) {
        if Self::is_hotbar_slot(slot as usize) {
            if self.selected != slot {
                self.selected = slot;
            }
        } else {
            panic!("Invalid hotbar slot: {slot}");
        }
    }

    /// Sets the selected hotbar slot from the signed protocol field.
    ///
    /// Returns an error when the packet value is outside the vanilla hotbar
    /// range instead of wrapping or panicking.
    pub fn try_set_selected_slot_from_packet(
        &mut self,
        slot: i16,
    ) -> Result<(), InvalidHotbarSlot> {
        let Ok(slot) = u8::try_from(slot) else {
            return Err(InvalidHotbarSlot);
        };
        if !Self::is_hotbar_slot(slot as usize) {
            return Err(InvalidHotbarSlot);
        }

        self.set_selected_slot(slot);
        Ok(())
    }

    /// Executes a function with a reference to the currently selected item.
    pub fn with_selected_item<R>(&self, f: impl FnOnce(&ItemStack) -> R) -> R {
        f(&self.items[self.selected as usize])
    }

    /// Returns a mutable reference to the currently selected item (main hand).
    #[must_use]
    pub const fn get_selected_item(&self) -> &ItemStack {
        &self.items[self.selected as usize]
    }

    /// Returns the currently selected item (main hand).
    pub fn get_selected_item_mut(&mut self) -> &mut ItemStack {
        EntityEquipment::get_mut(self, EquipmentSlot::MainHand)
    }

    /// Sets the currently selected item (main hand).
    pub fn set_selected_item(&mut self, item: ItemStack) {
        let _ = EntityEquipment::set(self, EquipmentSlot::MainHand, item);
    }

    /// Returns the offhand item.
    #[must_use]
    pub fn get_offhand_item(&self) -> &ItemStack {
        EntityEquipment::get_ref(self, EquipmentSlot::OffHand)
    }

    /// Returns a mutable reference to the offhand item.
    pub fn get_offhand_item_mut(&mut self) -> &mut ItemStack {
        EntityEquipment::get_mut(self, EquipmentSlot::OffHand)
    }

    /// Sets the offhand item.
    pub fn set_offhand_item(&mut self, item: ItemStack) {
        let _ = EntityEquipment::set(self, EquipmentSlot::OffHand, item);
    }

    /// Executes a function with a mutable reference to the currently selected item.
    pub fn with_selected_item_mut<R>(&mut self, f: impl FnOnce(&mut ItemStack) -> R) -> R {
        self.with_equipment_item_mut(EquipmentSlot::MainHand, f)
    }

    pub(in crate::player) fn with_equipment_item_mut<R>(
        &mut self,
        slot: EquipmentSlot,
        f: impl FnOnce(&mut ItemStack) -> R,
    ) -> R {
        let inventory_index = self.equipment_slot_index(slot);
        let previous = self.items[inventory_index].clone();
        let result = f(&mut self.items[inventory_index]);
        if !ItemStack::matches(&self.items[inventory_index], &previous) {
            Container::set_changed(self);
        }
        result
    }

    /// Returns the number of times this inventory has been modified.
    #[must_use]
    pub const fn get_times_changed(&self) -> u32 {
        self.times_changed
    }

    /// Returns the non-equipment items (main 36 slots).
    #[must_use]
    pub fn get_items(&self) -> &[ItemStack; Self::INVENTORY_SIZE] {
        let Some(items) = self.items.first_chunk::<{ Self::INVENTORY_SIZE }>() else {
            unreachable!("the player inventory always contains its 36 main slots");
        };
        items
    }

    /// Finds the first empty slot in the inventory, or -1 if full.
    #[must_use]
    pub fn get_free_slot(&self) -> i32 {
        for i in 0..Self::INVENTORY_SIZE {
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
        for i in 0..Self::INVENTORY_SIZE {
            if !self.items[i].is_empty() && ItemStack::is_same_item(&self.items[i], stack) {
                return i as i32;
            }
        }
        -1
    }

    /// Finds a slot containing an item matching the exact given stack (same item type).
    /// Returns -1 if not found.
    #[must_use]
    pub fn find_slot_matching_item_with_same_componnents(&self, stack: &ItemStack) -> i32 {
        for i in 0..Self::INVENTORY_SIZE {
            if !self.items[i].is_empty()
                && ItemStack::is_same_item_same_components(&self.items[i], stack)
            {
                return i as i32;
            }
        }
        -1
    }

    /// Swaps items between selected hotbar slot and the given slot.
    /// Used for pick block when item is in main inventory but not hotbar.
    pub fn pick_slot(&mut self, slot: i32) {
        let slot = slot as usize;
        if slot >= Self::INVENTORY_SIZE {
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
}
