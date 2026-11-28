use std::{array, ops::Range};

use simdnbt::owned::{NbtList, NbtTag};
use steel_registry::{enchantment_effect::EnchantmentEffectComponent, item_stack::ItemStack};
use steel_utils::{DowncastType, DowncastTypeKey, types::InteractionHand};

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
    /// Per-hand counters bumped when a hand slot is replaced, indexed by `InteractionHand`.
    pub(super) hand_write_generation: [u32; 2],
    /// Hand generations adopted as vanilla `LivingEntity.useItem`, which is compared by identity.
    use_item_generation: [u32; 2],
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
    pub const SELECTION_SIZE: u8 = 9;
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
            hand_write_generation: [0; 2],
            use_item_generation: [0; 2],
        }
    }

    /// Copies this inventory into a fresh player inventory for player replacement.
    #[must_use]
    pub(crate) fn replacement_copy(&self) -> Self {
        Self {
            items: array::from_fn(|slot| {
                let item = &self.items[slot];
                item.copy_with_count(item.count())
            }),
            selected: self.selected,
            times_changed: 0,
            hand_write_generation: [0; 2],
            use_item_generation: [0; 2],
        }
    }

    /// Returns true if the given slot index is a hotbar slot (0-8).
    #[must_use]
    pub const fn is_hotbar_slot(slot: u8) -> bool {
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
        if Self::is_hotbar_slot(slot) {
            if self.selected != slot {
                self.selected = slot;
                // A different hotbar stack is now in the main hand.
                self.mark_hand_written(slot as usize);
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
        if !Self::is_hotbar_slot(slot) {
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

    /// Adopts the stack in `hand` as the used item, like vanilla `useItem = getItemInHand(hand)`.
    pub(in crate::player) const fn adopt_use_item(&mut self, hand: InteractionHand) {
        self.use_item_generation[hand as usize] = self.hand_write_generation[hand as usize];
    }

    /// Returns whether `hand` still holds the adopted used item.
    #[must_use]
    pub(crate) const fn holds_use_item(&self, hand: InteractionHand) -> bool {
        self.use_item_generation[hand as usize] == self.hand_write_generation[hand as usize]
    }

    /// Writes back in-place changes to the used item, which vanilla makes directly
    /// on the hand stack; they are lost if the hand stack was replaced meanwhile.
    pub(in crate::player) fn write_use_item(&mut self, hand: InteractionHand, item: ItemStack) {
        if self.holds_use_item(hand) {
            self.mutate_item_in_hand(hand, |current| *current = item);
            self.adopt_use_item(hand);
        }
    }

    /// Vanilla `if (result != useItem) setItemInHand(hand, result)`.
    pub(in crate::player) fn write_use_result(
        &mut self,
        hand: InteractionHand,
        used: ItemStack,
        result: ItemStack,
    ) {
        if result == used {
            self.write_use_item(hand, used);
        } else {
            self.set_item_in_hand(hand, result);
        }
    }

    /// Bumps the hand generation if `index` is the main-hand or off-hand slot.
    pub(super) const fn mark_hand_written(&mut self, index: usize) {
        let hand = if index == self.selected as usize {
            InteractionHand::MainHand
        } else if index == Self::SLOT_OFFHAND {
            InteractionHand::OffHand
        } else {
            return;
        };
        let generation = &mut self.hand_write_generation[hand as usize];
        *generation = generation.wrapping_add(1);
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
        self.with_item_mut(self.equipment_slot_index(slot), f)
    }

    pub(crate) fn with_item_mut<R>(
        &mut self,
        index: usize,
        f: impl FnOnce(&mut ItemStack) -> R,
    ) -> R {
        let previous = self.items[index].clone();
        let result = f(&mut self.items[index]);
        if !ItemStack::matches(&self.items[index], &previous) {
            self.mark_hand_written(index);
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

    /// Finds the first empty slot in the inventory, or `None` if full.
    #[must_use]
    pub fn get_free_slot(&self) -> Option<u8> {
        (0..Self::INVENTORY_SIZE)
            .find(|&i| self.items[i].is_empty())
            .map(|i| i as u8)
    }

    /// Finds next empty slot in hotbar (and returns)
    /// if none, then
    ///     it finds the next slot with a non-enchanted item (and returns)
    /// if none, then
    ///     returns the current slot
    #[must_use]
    pub fn get_suitable_hotbar_slot(&self) -> u8 {
        for slot in 0..Self::SELECTION_SIZE {
            let index = (self.selected + slot) % Self::SELECTION_SIZE;
            if self.items[usize::from(index)].is_empty() {
                return index;
            }
        }

        for slot in 0..Self::SELECTION_SIZE {
            let index = (self.selected + slot) % Self::SELECTION_SIZE;
            if !self.items[usize::from(index)].is_enchanted() {
                return index;
            }
        }

        self.selected
    }

    /// Finds a slot containing an item matching the given stack (same item type).
    #[must_use]
    pub fn find_slot_matching_item(&self, stack: &ItemStack) -> Option<u8> {
        (0..Self::INVENTORY_SIZE)
            .find(|&i| !self.items[i].is_empty() && ItemStack::is_same_item(&self.items[i], stack))
            .map(|i| i as u8)
    }

    /// Finds a slot containing an item matching the exact given stack (same item type).
    #[must_use]
    pub fn find_slot_matching_item_with_same_components(&self, stack: &ItemStack) -> Option<u8> {
        (0..Self::INVENTORY_SIZE)
            .find(|&i| {
                !self.items[i].is_empty()
                    && ItemStack::is_same_item_same_components(&self.items[i], stack)
            })
            .map(|i| i as u8)
    }

    /// Swaps items between selected hotbar slot and the given slot.
    /// Used for pick block when item is in main inventory but not hotbar.
    pub fn pick_slot(&mut self, slot: u8) {
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
            let j = usize::from(i);
            if self.items[j].is_empty() {
                self.items[j] = stack;
                self.selected = i;
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
