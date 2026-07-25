use std::sync::LazyLock;

use steel_registry::item_stack::ItemStack;
use steel_utils::types::InteractionHand;

use crate::inventory::{container::Container, equipment::EquipmentSlot};

use super::core::PlayerInventory;

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

impl PlayerInventory {
    /// Applies vanilla `ItemUtils.createFilledResult` to a held item.
    ///
    /// Mutates the held stack and inventory, returning only the result stack that
    /// should be dropped by the caller. Creative inventory insertion discards
    /// leftover result items instead of dropping them.
    pub fn apply_filled_result(
        &mut self,
        hand: InteractionHand,
        mut result_stack: ItemStack,
        has_infinite_materials: bool,
        limit_creative_stack_size: bool,
    ) -> ItemStack {
        if limit_creative_stack_size && has_infinite_materials {
            if !self.contains_stack(&result_stack) {
                let _ = self.add(&mut result_stack);
            }
            return ItemStack::empty();
        }

        if !has_infinite_materials {
            self.shrink_item_in_hand(hand, 1);
        }

        if self.get_item_in_hand(hand).is_empty() {
            self.set_item_in_hand(hand, result_stack);
            return ItemStack::empty();
        }

        let added = self.add(&mut result_stack);
        if added || has_infinite_materials {
            ItemStack::empty()
        } else {
            result_stack
        }
    }
}

/// Static empty item stack for returning references to invalid slots.
static EMPTY_ITEM: LazyLock<ItemStack> = LazyLock::new(ItemStack::empty);

/// Error returned when a carried-item packet selects a non-hotbar slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidHotbarSlot;

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
        let mut changed = false;

        // First pass: try to stack with existing items in main inventory only
        if stack.is_stackable() {
            for slot in 0..Self::INVENTORY_SIZE {
                if stack.is_empty() {
                    if changed {
                        self.set_changed();
                    }
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
                        if slot == self.selected as usize {
                            self.mark_main_hand_dirty();
                        }
                        changed = true;
                    }
                }
            }
        }

        // Second pass: try empty slots in main inventory only
        for slot in 0..Self::INVENTORY_SIZE {
            if stack.is_empty() {
                if changed {
                    self.set_changed();
                }
                return true;
            }
            if self.items[slot].is_empty() {
                let to_place = stack.count().min(max_size);
                self.items[slot] = stack.split(to_place);
                if slot == self.selected as usize {
                    self.mark_main_hand_dirty();
                    self.refresh_player_equipment_attribute_modifiers(EquipmentSlot::MainHand);
                }
                changed = true;
            }
        }

        if changed {
            self.set_changed();
        }
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
            if slot == self.selected as usize {
                self.mark_main_hand_dirty();
            }
            &mut self.items[slot]
        } else if let Some(eq_slot) = slot_to_equipment(slot) {
            self.equipment.get_mut(eq_slot)
        } else {
            panic!("Invalid slot index: {slot}");
        }
    }

    fn set_item(&mut self, slot: usize, stack: ItemStack) {
        if slot < Self::INVENTORY_SIZE {
            let refresh_main_hand = slot == self.selected as usize && self.items[slot] != stack;
            if refresh_main_hand {
                self.mark_main_hand_dirty();
            }
            self.items[slot] = stack;
            if refresh_main_hand {
                self.refresh_player_equipment_attribute_modifiers(EquipmentSlot::MainHand);
            }
        } else if let Some(eq_slot) = slot_to_equipment(slot) {
            let old = self.equipment.set(eq_slot, stack);
            if old != *self.equipment.get_ref(eq_slot) {
                self.refresh_player_equipment_attribute_modifiers(eq_slot);
            }
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
        let selected = self.selected as usize;
        if !self.items[selected].is_empty() {
            self.mark_main_hand_dirty();
        }
        for item in &mut self.items {
            count += item.count();
            *item = ItemStack::empty();
        }
        for slot in EquipmentSlot::ALL {
            count += self.equipment.get_ref(slot).count();
        }
        self.equipment.clear();
        self.refresh_player_equipment_attribute_modifiers(EquipmentSlot::MainHand);
        for slot in EquipmentSlot::ALL {
            if slot != EquipmentSlot::MainHand {
                self.refresh_player_equipment_attribute_modifiers(slot);
            }
        }
        if count > 0 {
            self.set_changed();
        }
        count
    }

    fn clear_content_matching(&mut self, predicate: &mut dyn FnMut(&mut ItemStack) -> bool) -> i32 {
        let mut count = 0;
        let selected = self.selected as usize;
        let mut main_hand_changed = false;
        let mut equipment_changed = [false; 8];
        for slot in 0..Self::INVENTORY_SIZE {
            if predicate(&mut self.items[slot]) {
                if slot == selected {
                    self.mark_main_hand_dirty();
                    main_hand_changed = true;
                }
                count += self.items[slot].count();
                self.items[slot] = ItemStack::empty();
            }
        }
        for slot in EquipmentSlot::ALL {
            let item = self.equipment.get_mut(slot);
            if predicate(item) {
                count += item.count();
                *item = ItemStack::empty();
                equipment_changed[slot.index()] = true;
            }
        }
        if main_hand_changed {
            self.refresh_player_equipment_attribute_modifiers(EquipmentSlot::MainHand);
        }
        for slot in EquipmentSlot::ALL {
            if equipment_changed[slot.index()] {
                self.refresh_player_equipment_attribute_modifiers(slot);
            }
        }
        if count > 0 {
            self.set_changed();
        }
        count
    }
}

impl PlayerInventory {
    pub(super) const fn mark_main_hand_dirty(&mut self) {
        self.dirty_main_hand = true;
    }
}
