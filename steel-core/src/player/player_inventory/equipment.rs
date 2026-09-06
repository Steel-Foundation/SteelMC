use std::mem;

use steel_registry::{
    REGISTRY, RegistryExt, enchantment_effect::EnchantmentEffectComponent, item_stack::ItemStack,
    items::ItemRef,
};
use steel_utils::types::InteractionHand;

use crate::inventory::{
    container::Container,
    equipment::{EntityEquipment, EquipmentSlot},
};

use super::core::PlayerInventory;

/// Result of swapping a held item with an equipment slot.
#[derive(Debug, PartialEq)]
pub enum EquipmentSwapResult {
    /// The swap succeeded. Contains an overflow stack that should be dropped if non-empty.
    Success(ItemStack),
    /// The swap is blocked by vanilla equipment rules.
    Fail,
}

const fn hand_to_equipment_slot(hand: InteractionHand) -> EquipmentSlot {
    match hand {
        InteractionHand::MainHand => EquipmentSlot::MainHand,
        InteractionHand::OffHand => EquipmentSlot::OffHand,
    }
}

const fn equipment_to_slot(slot: EquipmentSlot, selected: u8) -> usize {
    match slot {
        EquipmentSlot::MainHand => selected as usize,
        EquipmentSlot::OffHand => 40,
        EquipmentSlot::Feet => 36,
        EquipmentSlot::Legs => 37,
        EquipmentSlot::Chest => 38,
        EquipmentSlot::Head => 39,
        EquipmentSlot::Body => 41,
        EquipmentSlot::Saddle => 42,
    }
}

impl PlayerInventory {
    /// Gets the item in the specified hand.
    #[must_use]
    pub fn get_item_in_hand(&self, hand: InteractionHand) -> &ItemStack {
        match hand {
            InteractionHand::MainHand => self.get_selected_item(),
            InteractionHand::OffHand => self.get_offhand_item(),
        }
    }

    /// Gets the item in the specified hand.
    #[must_use]
    pub fn get_item_in_hand_mut(&mut self, hand: InteractionHand) -> &mut ItemStack {
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

    /// Shrinks the item in the specified hand and records inventory/equipment changes.
    pub fn shrink_item_in_hand(&mut self, hand: InteractionHand, amount: i32) {
        if amount <= 0 || self.get_item_in_hand(hand).is_empty() {
            return;
        }

        self.get_item_in_hand_mut(hand).shrink(amount);
        self.set_changed();
    }

    /// Splits items from the specified hand and records inventory/equipment changes.
    pub fn split_item_in_hand(&mut self, hand: InteractionHand, amount: i32) -> ItemStack {
        if amount <= 0 || self.get_item_in_hand(hand).is_empty() {
            return ItemStack::empty();
        }

        let result = self.get_item_in_hand_mut(hand).split(amount);
        self.set_changed();
        result
    }

    /// Damages the held item and records inventory/equipment changes.
    pub fn hurt_item_in_hand(
        &mut self,
        hand: InteractionHand,
        amount: i32,
        has_infinite_materials: bool,
    ) {
        if amount <= 0 || self.get_item_in_hand(hand).is_empty() {
            return;
        }

        let changed = {
            let item = self.get_item_in_hand_mut(hand);
            let previous_item = item.item();
            let previous_count = item.count();
            let previous_damage = item.get_damage_value();

            let _ = item.hurt_and_break(amount, has_infinite_materials);

            item.item() != previous_item
                || item.count() != previous_count
                || item.get_damage_value() != previous_damage
        };

        if changed {
            self.set_changed();
        }
    }

    /// Mutates the held item and records inventory/equipment changes if its stack state changed.
    pub fn mutate_item_in_hand<R>(
        &mut self,
        hand: InteractionHand,
        f: impl FnOnce(&mut ItemStack) -> R,
    ) -> R {
        self.with_equipment_item_mut(hand_to_equipment_slot(hand), f)
    }

    /// Damages the held item and converts it to `replacement_item` if it breaks.
    ///
    /// Mirrors vanilla `ItemStack.hurtAndConvertOnBreak` for hand-held player items.
    pub fn hurt_and_convert_item_in_hand_on_break(
        &mut self,
        hand: InteractionHand,
        amount: i32,
        replacement_item: ItemRef,
        has_infinite_materials: bool,
    ) {
        if amount <= 0 || self.get_item_in_hand(hand).is_empty() {
            return;
        }

        let changed = {
            let item = self.get_item_in_hand_mut(hand);
            let previous_item = item.item();
            let previous_count = item.count();
            let previous_damage = item.get_damage_value();

            if item.hurt_and_break(amount, has_infinite_materials) && item.is_empty() {
                item.set_item(&replacement_item.key);
                item.set_count(1);
                if item.is_damageable_item() {
                    item.set_damage_value(0);
                }
            }

            item.item() != previous_item
                || item.count() != previous_count
                || item.get_damage_value() != previous_damage
        };

        if changed {
            self.set_changed();
        }
    }

    /// Swaps the selected main-hand item with the offhand item.
    ///
    /// Returns true when the visible hand contents changed.
    pub fn swap_hands(&mut self) -> bool {
        if ItemStack::matches(self.get_selected_item(), self.get_offhand_item()) {
            return false;
        }

        let main_hand = EntityEquipment::take(self, EquipmentSlot::MainHand);
        let offhand = EntityEquipment::take(self, EquipmentSlot::OffHand);
        let _ = EntityEquipment::set(self, EquipmentSlot::MainHand, offhand);
        let _ = EntityEquipment::set(self, EquipmentSlot::OffHand, main_hand);
        true
    }

    /// Attempts to equip the held item into the target equipment slot.
    pub fn try_swap_with_equipment_slot(
        &mut self,
        hand: InteractionHand,
        slot: EquipmentSlot,
        has_infinite_materials: bool,
    ) -> EquipmentSwapResult {
        let in_hand = self.get_item_in_hand(hand);
        if in_hand.is_empty() {
            return EquipmentSwapResult::Fail;
        }

        let in_equipment_slot = EntityEquipment::get_ref(self, slot);
        if ItemStack::is_same_item_same_components(in_hand, in_equipment_slot) {
            return EquipmentSwapResult::Fail;
        }

        if !has_infinite_materials
            && in_equipment_slot
                .has_enchantment_effect(EnchantmentEffectComponent::PreventArmorChange)
        {
            return EquipmentSwapResult::Fail;
        }

        if in_hand.count() <= 1 {
            self.swap_single_item_with_equipment_slot(hand, slot, has_infinite_materials);
            return EquipmentSwapResult::Success(ItemStack::empty());
        }

        let to_equip = self
            .get_item_in_hand_mut(hand)
            .consume_and_return(1, has_infinite_materials);
        let mut overflow = EntityEquipment::set(self, slot, to_equip);
        if !overflow.is_empty() && self.add(&mut overflow) {
            overflow = ItemStack::empty();
        }

        EquipmentSwapResult::Success(overflow)
    }

    /// Repairs a random damaged equipped item with `REPAIR_WITH_XP`, returning leftover XP.
    pub fn repair_random_equipped_item_with_xp(&mut self, amount: i32) -> i32 {
        let mut remaining = amount;

        loop {
            let candidates = self.repair_with_xp_candidate_slots();
            if candidates.is_empty() {
                return remaining;
            }

            let selected = rand::random_range(0..candidates.len());
            let slot = candidates[selected];
            let item = EntityEquipment::get_mut(self, slot);
            let to_repair = item
                .apply_unconditional_enchantment_value_effects(
                    EnchantmentEffectComponent::RepairWithXp,
                    remaining as f32,
                )
                .max(0.0) as i32;
            if to_repair <= 0 {
                return 0;
            }

            let damage = item.get_damage_value();
            let repair = to_repair.min(damage);
            if repair <= 0 {
                return 0;
            }

            item.set_damage_value(damage - repair);
            self.set_changed();

            remaining -= repair * remaining / to_repair;
            if remaining <= 0 {
                return 0;
            }
        }
    }

    fn swap_single_item_with_equipment_slot(
        &mut self,
        hand: InteractionHand,
        slot: EquipmentSlot,
        has_infinite_materials: bool,
    ) {
        if has_infinite_materials {
            let held = self
                .get_item_in_hand(hand)
                .copy_with_count(self.get_item_in_hand(hand).count());
            let previous = EntityEquipment::set(self, slot, held);
            if !previous.is_empty() {
                self.set_item_in_hand(hand, previous);
            }
            return;
        }

        let held = self.take_item_in_hand(hand);
        let previous = EntityEquipment::set(self, slot, held);
        self.set_item_in_hand(hand, previous);
    }

    fn repair_with_xp_candidate_slots(&self) -> Vec<EquipmentSlot> {
        let mut slots = Vec::new();
        for slot in EquipmentSlot::ALL {
            let item = EntityEquipment::get_ref(self, slot);
            if !item.is_damaged() {
                continue;
            }

            let Some(enchantments) = item.get_enchantments() else {
                continue;
            };
            for (key, level) in enchantments.iter() {
                if *level == 0 {
                    continue;
                }
                let Some(enchantment) = REGISTRY.enchantments.by_key(key) else {
                    continue;
                };
                if enchantment
                    .effects
                    .has(EnchantmentEffectComponent::RepairWithXp)
                    && enchantment.matching_slot(slot)
                {
                    slots.push(slot);
                }
            }
        }
        slots
    }

    fn take_item_in_hand(&mut self, hand: InteractionHand) -> ItemStack {
        match hand {
            InteractionHand::MainHand => EntityEquipment::take(self, EquipmentSlot::MainHand),
            InteractionHand::OffHand => EntityEquipment::take(self, EquipmentSlot::OffHand),
        }
    }
}

impl PlayerInventory {
    pub(super) const fn equipment_slot_index(&self, slot: EquipmentSlot) -> usize {
        equipment_to_slot(slot, self.selected)
    }
}

impl EntityEquipment for PlayerInventory {
    fn get_ref(&self, slot: EquipmentSlot) -> &ItemStack {
        &self.items[self.equipment_slot_index(slot)]
    }

    fn get_mut(&mut self, slot: EquipmentSlot) -> &mut ItemStack {
        let inventory_index = self.equipment_slot_index(slot);
        &mut self.items[inventory_index]
    }

    fn set(&mut self, slot: EquipmentSlot, stack: ItemStack) -> ItemStack {
        let inventory_index = self.equipment_slot_index(slot);
        let old = mem::replace(&mut self.items[inventory_index], stack);
        Container::set_changed(self);
        old
    }

    fn take(&mut self, slot: EquipmentSlot) -> ItemStack {
        let inventory_index = self.equipment_slot_index(slot);
        let old = mem::take(&mut self.items[inventory_index]);
        if !old.is_empty() {
            Container::set_changed(self);
        }
        old
    }

    fn clear(&mut self) {
        let mut changed = false;
        for slot in EquipmentSlot::ALL {
            let inventory_index = self.equipment_slot_index(slot);
            if self.items[inventory_index].is_empty() {
                continue;
            }

            self.items[inventory_index] = ItemStack::empty();
            changed = true;
        }
        if changed {
            Container::set_changed(self);
        }
    }
}
