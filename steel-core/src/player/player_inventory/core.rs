use std::{array, sync::Weak};

use simdnbt::owned::{NbtList, NbtTag};
use steel_registry::item_stack::ItemStack;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::{
    inventory::{
        container::Container,
        equipment::{EntityEquipment, EquipmentSlot},
    },
    player::Player,
};

use super::container::InvalidHotbarSlot;

/// Player inventory container managing the main inventory and equipment.
///
/// Contains 36 main inventory slots (0-8 hotbar, 9-35 main) plus equipment slots
/// (armor, offhand, etc.) accessed through the Container trait.
pub struct PlayerInventory {
    /// The 36 main inventory slots (0-8 hotbar, 9-35 main).
    pub(super) items: [ItemStack; Self::INVENTORY_SIZE],
    /// Entity equipment (armor, hands).
    pub(super) equipment: EntityEquipment,
    /// Whether the selected hotbar item must be synced as main-hand equipment.
    pub(super) dirty_main_hand: bool,
    /// Weak reference to the player.
    pub(super) player: Weak<Player>,
    /// Currently selected hotbar slot (0-8).
    pub(super) selected: u8,
    /// Counter incremented on every change.
    pub(super) times_changed: u32,
}

// SAFETY: This key is owned by Steel and uniquely identifies `PlayerInventory`.
unsafe impl DowncastType for PlayerInventory {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/player_inventory");
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
            dirty_main_hand: false,
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

    /// Serializes the main inventory with vanilla's `ItemStackWithSlot` shape.
    #[must_use]
    pub(crate) fn to_vanilla_inventory_nbt(&self) -> NbtList {
        let items = self
            .items
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
                self.mark_main_hand_dirty();
                self.refresh_player_equipment_attribute_modifiers(EquipmentSlot::MainHand);
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
    pub const fn get_selected_item_mut(&mut self) -> &mut ItemStack {
        self.mark_main_hand_dirty();
        &mut self.items[self.selected as usize]
    }

    /// Sets the currently selected item (main hand).
    pub fn set_selected_item(&mut self, item: ItemStack) {
        let changed = self.items[self.selected as usize] != item;
        if changed {
            self.mark_main_hand_dirty();
        }
        self.items[self.selected as usize] = item;
        if changed {
            self.refresh_player_equipment_attribute_modifiers(EquipmentSlot::MainHand);
        }
        self.set_changed();
    }

    /// Returns the offhand item.
    #[must_use]
    pub const fn get_offhand_item(&self) -> &ItemStack {
        self.equipment.get_ref(EquipmentSlot::OffHand)
    }

    /// Returns a mutable reference to the offhand item.
    pub const fn get_offhand_item_mut(&mut self) -> &mut ItemStack {
        self.equipment.get_mut(EquipmentSlot::OffHand)
    }

    /// Sets the offhand item.
    pub fn set_offhand_item(&mut self, item: ItemStack) {
        let old = self.equipment.set(EquipmentSlot::OffHand, item);
        if old != *self.equipment.get_ref(EquipmentSlot::OffHand) {
            self.refresh_player_equipment_attribute_modifiers(EquipmentSlot::OffHand);
        }
        self.set_changed();
    }

    /// Executes a function with a mutable reference to the currently selected item.
    pub fn with_selected_item_mut<R>(&mut self, f: impl FnOnce(&mut ItemStack) -> R) -> R {
        let previous = self.items[self.selected as usize].clone();
        let result = f(&mut self.items[self.selected as usize]);
        if self.items[self.selected as usize] != previous {
            self.mark_main_hand_dirty();
            self.refresh_player_equipment_attribute_modifiers(EquipmentSlot::MainHand);
        }
        self.set_changed();
        result
    }

    /// Returns non-empty equipment slots for entity tracking spawn sync.
    #[must_use]
    pub fn non_empty_equipment_items(&self) -> Vec<(EquipmentSlot, ItemStack)> {
        let mut items = Vec::new();
        let main_hand = self.get_selected_item();
        if !main_hand.is_empty() {
            items.push((EquipmentSlot::MainHand, main_hand.clone()));
        }
        items.extend(
            self.equipment
                .non_empty_items()
                .into_iter()
                .filter(|(slot, _)| *slot != EquipmentSlot::MainHand),
        );
        items
    }

    /// Drains equipment slots that changed since the last entity tracking sync.
    pub fn drain_dirty_equipment_items(&mut self) -> Vec<(EquipmentSlot, ItemStack)> {
        let mut items = Vec::new();
        if self.dirty_main_hand {
            self.dirty_main_hand = false;
            items.push((EquipmentSlot::MainHand, self.get_selected_item().clone()));
        }
        items.extend(
            self.equipment
                .drain_dirty_items()
                .into_iter()
                .filter(|(slot, _)| *slot != EquipmentSlot::MainHand),
        );
        items
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
        if selected != slot {
            self.mark_main_hand_dirty();
        }
        self.items.swap(selected, slot);
        if selected != slot {
            self.refresh_player_equipment_attribute_modifiers(EquipmentSlot::MainHand);
        }
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
                self.mark_main_hand_dirty();
                self.refresh_player_equipment_attribute_modifiers(EquipmentSlot::MainHand);
                self.set_changed();
                return true;
            }
        }
        // No empty slot, replace current slot
        self.items[self.selected as usize] = stack;
        self.mark_main_hand_dirty();
        self.refresh_player_equipment_attribute_modifiers(EquipmentSlot::MainHand);
        self.set_changed();
        true
    }
}
