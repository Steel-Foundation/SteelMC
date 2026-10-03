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

/// Result of preparing a held-item equipment swap at the equip-hook boundary.
#[derive(Debug, PartialEq)]
#[must_use]
pub enum EquipmentSwapResult {
    /// Equipment is installed; announce the equip before finishing the swap.
    Success(PreparedEquipmentSwap),
    /// The swap is blocked by vanilla equipment rules.
    Fail,
}

/// An installed equipment change whose old stack has not yet been returned.
#[derive(Debug, PartialEq)]
#[must_use]
pub struct PreparedEquipmentSwap {
    previous: ItemStack,
    equipped: ItemStack,
    return_to: EquipmentSwapReturn,
}

#[derive(Debug, PartialEq)]
enum EquipmentSwapReturn {
    Hand(InteractionHand),
    Inventory,
    KeepHand,
}

impl PreparedEquipmentSwap {
    /// The equipment replaced by this swap.
    #[must_use]
    pub const fn previous(&self) -> &ItemStack {
        &self.previous
    }

    /// The stack installed before the equip hook.
    #[must_use]
    pub const fn equipped(&self) -> &ItemStack {
        &self.equipped
    }

    /// Returns old equipment after the equip hook, leaving any overflow for dropping.
    pub fn finish(self, inventory: &mut PlayerInventory) -> ItemStack {
        match self.return_to {
            EquipmentSwapReturn::Hand(hand) => {
                inventory.set_item_in_hand(hand, self.previous);
                ItemStack::empty()
            }
            EquipmentSwapReturn::Inventory => {
                let mut overflow = self.previous;
                if !overflow.is_empty() && inventory.add(&mut overflow) {
                    overflow = ItemStack::empty();
                }
                overflow
            }
            EquipmentSwapReturn::KeepHand => ItemStack::empty(),
        }
    }
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

    /// Installs equipment; run the equip hook unlocked before finishing the swap.
    pub fn prepare_equipment_swap(
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

        let single_item = in_hand.count() <= 1;
        let to_equip = if single_item {
            if has_infinite_materials {
                in_hand.copy_with_count(in_hand.count())
            } else {
                self.take_item_in_hand(hand)
            }
        } else {
            self.get_item_in_hand_mut(hand)
                .consume_and_return(1, has_infinite_materials)
        };
        let equipped = to_equip.copy_with_count(to_equip.count());
        let previous = EntityEquipment::set(self, slot, to_equip);
        let return_to = if !single_item {
            EquipmentSwapReturn::Inventory
        } else if has_infinite_materials && previous.is_empty() {
            EquipmentSwapReturn::KeepHand
        } else {
            EquipmentSwapReturn::Hand(hand)
        };
        EquipmentSwapResult::Success(PreparedEquipmentSwap {
            previous,
            equipped,
            return_to,
        })
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
