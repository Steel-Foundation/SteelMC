//! Player inventory management.

use std::{
    array,
    f32::consts::TAU,
    mem,
    sync::{LazyLock, Weak},
};

use glam::DVec3;
use steel_protocol::packets::game::{
    CContainerClose, COpenScreen, SContainerButtonClick, SContainerClick, SContainerClose,
    SContainerSlotStateChanged, SSetCarriedItem, SSetCreativeModeSlot,
};
use steel_registry::enchantment_effect::EnchantmentEffectComponent;
use steel_registry::item_stack::ItemStack;
use steel_registry::{REGISTRY, RegistryExt, items::ItemRef};
use steel_utils::types::{GameType, InteractionHand};

use crate::{
    entity::Entity,
    inventory::{
        MenuProvider,
        container::Container,
        equipment::{EntityEquipment, EquipmentSlot},
        inventory_menu::InventoryMenu,
        lock::{ContainerId, ContainerLockGuard},
        menu::Menu,
        slot::Slot,
    },
    player::Player,
};

/// Result of swapping a held item with an equipment slot.
#[derive(Debug, PartialEq)]
pub enum EquipmentSwapResult {
    /// The swap succeeded. Contains an overflow stack that should be dropped if non-empty.
    Success(ItemStack),
    /// The swap is blocked by vanilla equipment rules.
    Fail,
}

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

const fn hand_to_equipment_slot(hand: InteractionHand) -> EquipmentSlot {
    match hand {
        InteractionHand::MainHand => EquipmentSlot::MainHand,
        InteractionHand::OffHand => EquipmentSlot::OffHand,
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
    /// Whether the selected hotbar item must be synced as main-hand equipment.
    dirty_main_hand: bool,
    /// Weak reference to the player.
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

    /// Shrinks the item in the specified hand and records inventory/equipment changes.
    pub fn shrink_item_in_hand(&mut self, hand: InteractionHand, amount: i32) {
        if amount <= 0 || self.get_item_in_hand(hand).is_empty() {
            return;
        }

        self.get_item_in_hand_mut(hand).shrink(amount);
        let slot = match hand {
            InteractionHand::MainHand => EquipmentSlot::MainHand,
            InteractionHand::OffHand => EquipmentSlot::OffHand,
        };
        self.refresh_player_equipment_attribute_modifiers(slot);
        self.set_changed();
    }

    /// Splits items from the specified hand and records inventory/equipment changes.
    pub fn split_item_in_hand(&mut self, hand: InteractionHand, amount: i32) -> ItemStack {
        if amount <= 0 || self.get_item_in_hand(hand).is_empty() {
            return ItemStack::empty();
        }

        let result = self.get_item_in_hand_mut(hand).split(amount);
        let slot = hand_to_equipment_slot(hand);
        self.refresh_player_equipment_attribute_modifiers(slot);
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

        let slot = hand_to_equipment_slot(hand);
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
            self.refresh_player_equipment_attribute_modifiers(slot);
            self.set_changed();
        }
    }

    /// Mutates the held item and records inventory/equipment changes if its stack state changed.
    pub fn mutate_item_in_hand<R>(
        &mut self,
        hand: InteractionHand,
        f: impl FnOnce(&mut ItemStack) -> R,
    ) -> R {
        let slot = hand_to_equipment_slot(hand);
        let previous_item = self.get_item_in_hand(hand).item();
        let previous_count = self.get_item_in_hand(hand).count();
        let previous_damage = self.get_item_in_hand(hand).get_damage_value();

        let result = f(self.get_item_in_hand_mut(hand));

        let item = self.get_item_in_hand(hand);
        let changed = item.item() != previous_item
            || item.count() != previous_count
            || item.get_damage_value() != previous_damage;
        if changed {
            self.refresh_player_equipment_attribute_modifiers(slot);
            self.set_changed();
        }

        result
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

        let slot = hand_to_equipment_slot(hand);
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
            self.refresh_player_equipment_attribute_modifiers(slot);
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

        let main_hand = self.take_equipment_slot_item(EquipmentSlot::MainHand);
        let offhand = self.take_equipment_slot_item(EquipmentSlot::OffHand);
        self.set_equipment_slot_item(EquipmentSlot::MainHand, offhand);
        self.set_equipment_slot_item(EquipmentSlot::OffHand, main_hand);
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

        let in_equipment_slot = self.get_equipment_slot_item(slot);
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

        let to_equip = in_hand.copy_with_count(1);
        if !has_infinite_materials {
            self.get_item_in_hand_mut(hand).shrink(1);
        }
        let mut overflow = self.set_equipment_slot_item(slot, to_equip);
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
            let item = self.get_equipment_slot_item_mut(slot);
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
            let previous = self.set_equipment_slot_item(slot, held);
            if !previous.is_empty() {
                self.set_item_in_hand(hand, previous);
            }
            return;
        }

        let held = self.take_item_in_hand(hand);
        let previous = self.set_equipment_slot_item(slot, held);
        self.set_item_in_hand(hand, previous);
    }

    const fn get_equipment_slot_item(&self, slot: EquipmentSlot) -> &ItemStack {
        match slot {
            EquipmentSlot::MainHand => self.get_selected_item(),
            _ => self.equipment.get_ref(slot),
        }
    }

    const fn get_equipment_slot_item_mut(&mut self, slot: EquipmentSlot) -> &mut ItemStack {
        match slot {
            EquipmentSlot::MainHand => {
                self.mark_main_hand_dirty();
                &mut self.items[self.selected as usize]
            }
            _ => self.equipment.get_mut(slot),
        }
    }

    fn repair_with_xp_candidate_slots(&self) -> Vec<EquipmentSlot> {
        let mut slots = Vec::new();
        for slot in EquipmentSlot::ALL {
            let item = self.get_equipment_slot_item(slot);
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

    fn refresh_player_equipment_attribute_modifiers(&self, slot: EquipmentSlot) {
        let Some(player) = self.player.upgrade() else {
            return;
        };
        player.refresh_equipment_attribute_modifiers_from_stack(
            slot,
            self.get_equipment_slot_item(slot),
        );
    }

    fn set_equipment_slot_item(&mut self, slot: EquipmentSlot, item: ItemStack) -> ItemStack {
        if slot == EquipmentSlot::MainHand {
            return self.set_selected_equipment_item(item);
        }

        let old = self.equipment.set(slot, item);
        if old != *self.equipment.get_ref(slot) {
            self.refresh_player_equipment_attribute_modifiers(slot);
        }
        self.set_changed();
        old
    }

    fn set_selected_equipment_item(&mut self, item: ItemStack) -> ItemStack {
        let selected = self.selected as usize;
        let old = mem::replace(&mut self.items[selected], item);
        if old != self.items[selected] {
            self.mark_main_hand_dirty();
            self.refresh_player_equipment_attribute_modifiers(EquipmentSlot::MainHand);
        }
        self.set_changed();
        old
    }

    fn take_item_in_hand(&mut self, hand: InteractionHand) -> ItemStack {
        match hand {
            InteractionHand::MainHand => self.take_equipment_slot_item(EquipmentSlot::MainHand),
            InteractionHand::OffHand => self.take_equipment_slot_item(EquipmentSlot::OffHand),
        }
    }

    fn take_equipment_slot_item(&mut self, slot: EquipmentSlot) -> ItemStack {
        if slot == EquipmentSlot::MainHand {
            let selected = self.selected as usize;
            let old = mem::take(&mut self.items[selected]);
            if !old.is_empty() {
                self.mark_main_hand_dirty();
                self.refresh_player_equipment_attribute_modifiers(EquipmentSlot::MainHand);
                self.set_changed();
            }
            return old;
        }

        let old = self.equipment.take(slot);
        if !old.is_empty() {
            self.refresh_player_equipment_attribute_modifiers(slot);
            self.set_changed();
        }
        old
    }
}

impl Player {
    /// Attempts to pick up nearby item entities.
    ///
    /// Mirrors vanilla's `Player.aiStep()` item pickup logic:
    /// - Calculates pickup area as bounding box inflated by (1.0, 0.5, 1.0)
    /// - Calls `playerTouch()` on each entity in range
    pub(super) fn touch_nearby_items(&self) {
        if self.game_mode() == GameType::Spectator {
            return;
        }

        let pickup_area = self.bounding_box().inflate_xyz(1.0, 0.5, 1.0);
        let world = self.get_world();
        let entities = world.get_entities_in_aabb(&pickup_area);

        let Some(player_arc) = world.players.get_by_entity_id(self.id()) else {
            return;
        };

        for entity in entities {
            if entity.id() == self.id() || entity.is_removed() {
                continue;
            }

            entity.player_touch(&player_arc);
        }
    }

    /// Handles a container button click packet (e.g., enchanting table buttons).
    pub fn handle_container_button_click(&self, packet: SContainerButtonClick) {
        log::debug!(
            "Player {} clicked button {} in container {}",
            self.gameprofile.name,
            packet.button_id,
            packet.container_id
        );
        // TODO: Implement container button click handling
        // This is used for things like:
        // - Enchanting table level selection
        // - Stonecutter recipe selection
        // - Loom pattern selection
        // - Lectern page turning
    }

    /// Handles a container click packet (slot interaction).
    pub fn handle_container_click(&self, packet: SContainerClick) {
        let mut open_menu_guard = self.open_menu.lock();

        if let Some(ref mut menu) = *open_menu_guard {
            if i32::from(menu.container_id()) != packet.container_id {
                return;
            }

            self.process_container_click(menu.as_mut(), packet);
        } else {
            drop(open_menu_guard);
            let mut menu = self.inventory_menu.lock();

            if i32::from(menu.behavior().container_id) != packet.container_id {
                return;
            }

            self.process_container_click(&mut *menu, packet);
        }
    }

    /// Processes a container click on any menu implementing the Menu trait.
    ///
    /// This is the common implementation shared between inventory menu and
    /// external menus (crafting table, chest, etc.).
    fn process_container_click(&self, menu: &mut dyn Menu, packet: SContainerClick) {
        if self.game_mode() == GameType::Spectator {
            menu.behavior_mut()
                .send_all_data_to_remote(&self.connection);
            return;
        }

        if !menu.still_valid(self) {
            log::debug!(
                "Player {} interacted with invalid menu",
                self.gameprofile.name
            );
            return;
        }

        if !menu.behavior().is_valid_slot_index(packet.slot_num) {
            log::debug!(
                "Player {} clicked invalid slot index: {}, available: {}",
                self.gameprofile.name,
                packet.slot_num,
                menu.behavior().slot_count()
            );
            return;
        }

        let full_resync_needed = packet.state_id as u32 != menu.behavior().get_state_id();

        menu.behavior_mut().suppress_remote_updates();

        let has_infinite_materials = self.game_mode() == GameType::Creative;
        menu.clicked(
            packet.slot_num,
            packet.button_num,
            packet.click_type,
            has_infinite_materials,
            self,
        );

        for (slot, hash) in packet.changed_slots {
            menu.behavior_mut().set_remote_slot(slot as usize, hash);
        }

        menu.behavior_mut().set_remote_carried(packet.carried_item);
        menu.behavior_mut().resume_remote_updates();

        if full_resync_needed {
            menu.behavior_mut().broadcast_full_state(&self.connection);
        } else {
            menu.behavior_mut().broadcast_changes(&self.connection);
        }
    }

    /// Handles a container close packet.
    ///
    /// Based on Java's `ServerGamePacketListenerImpl::handleContainerClose`.
    pub fn handle_container_close(&self, packet: SContainerClose) {
        log::debug!(
            "Player {} closed container {}",
            self.gameprofile.name,
            packet.container_id
        );

        let open_menu = self.open_menu.lock();
        if let Some(ref menu) = *open_menu
            && i32::from(menu.container_id()) == packet.container_id
        {
            drop(open_menu);
            self.do_close_container();
            return;
        }
        drop(open_menu);

        if packet.container_id == i32::from(InventoryMenu::CONTAINER_ID) {
            let mut menu = self.inventory_menu.lock();
            menu.removed(self);
        }
    }

    /// Handles a container slot state changed packet (e.g., crafter slot toggle).
    pub fn handle_container_slot_state_changed(&self, packet: SContainerSlotStateChanged) {
        log::debug!(
            "Player {} changed slot {} state to {} in container {}",
            self.gameprofile.name,
            packet.slot_id,
            packet.new_state,
            packet.container_id
        );
        // TODO: Implement slot state change handling
        // This is used for the crafter block to enable/disable slots
    }

    /// Handles a creative mode slot set packet.
    pub fn handle_set_creative_mode_slot(&self, packet: SSetCreativeModeSlot) {
        if self.game_mode() != GameType::Creative {
            return;
        }

        let drop = packet.slot_num < 0;
        let item_stack = packet.item_stack;

        let valid_slot = packet.slot_num >= 1 && packet.slot_num <= 45;
        let valid_data = item_stack.is_empty() || item_stack.count <= item_stack.max_stack_size();

        if valid_slot && valid_data {
            let mut menu = self.inventory_menu.lock();
            let slot_index = packet.slot_num as usize;

            {
                let mut guard = menu.behavior().lock_all_containers();
                if let Some(slot) = menu.behavior().get_slot(slot_index) {
                    let previous = slot.get_item(&guard).clone();
                    slot.set_by_player(&mut guard, item_stack.clone(), &previous);
                }
            }
            menu.behavior_mut()
                .set_remote_slot_known(slot_index, &item_stack);
            menu.behavior_mut().broadcast_changes(&self.connection);
        } else if drop && valid_data {
            // TODO: Implement drop spam throttling
            // For now, just drop the item
            if !item_stack.is_empty() {
                // TODO: Actually drop the item into the world
                log::debug!(
                    "Player {} would drop {:?} in creative mode",
                    self.gameprofile.name,
                    item_stack
                );
            }
        }
    }

    /// Sets selected slot
    pub fn handle_set_carried_item(&self, packet: SSetCarriedItem) {
        if self
            .inventory
            .lock()
            .try_set_selected_slot_from_packet(packet.slot)
            .is_err()
        {
            log::warn!(
                "{} tried to set an invalid carried item",
                self.gameprofile.name
            );
        }
    }

    /// Sends all inventory slots to the client (full sync).
    /// This should be called when the player first joins.
    pub fn send_inventory_to_remote(&self) {
        self.inventory_menu
            .lock()
            .behavior_mut()
            .send_all_data_to_remote(&self.connection);
    }

    /// Generates the next container ID (1-100, wrapping around).
    ///
    /// Based on Java's `ServerPlayer::nextContainerCounter`.
    fn next_container_counter(&self) -> u8 {
        self.container_counter.lock().next()
    }

    /// Opens a menu for this player.
    ///
    /// Based on Java's `ServerPlayer::openMenu`.
    ///
    /// # Arguments
    /// * `provider` - The menu provider containing the title and factory
    pub fn open_menu(&self, provider: &impl MenuProvider) {
        self.do_close_container();

        let container_id = self.next_container_counter();
        let mut menu = provider.create(container_id);

        self.send_packet(COpenScreen {
            container_id: i32::from(menu.container_id()),
            menu_type: menu.menu_type(),
            title: provider.title(),
        });

        menu.behavior_mut()
            .send_all_data_to_remote(&self.connection);

        *self.open_menu.lock() = Some(menu);
    }

    /// Closes the currently open container and returns to the inventory menu.
    ///
    /// Based on Java's `ServerPlayer::closeContainer`.
    /// This sends a close packet to the client.
    pub fn close_container(&self) {
        let open_menu = self.open_menu.lock();
        if let Some(menu) = &*open_menu {
            self.send_packet(CContainerClose {
                container_id: i32::from(menu.container_id()),
            });
        }
        drop(open_menu);
        self.do_close_container();
    }

    /// Internal close container logic without sending a packet.
    ///
    /// Based on Java's `ServerPlayer::doCloseContainer`.
    /// Called when the client sends a close packet or when opening a new menu.
    pub fn do_close_container(&self) {
        let mut open_menu = self.open_menu.lock();
        if let Some(ref mut menu) = *open_menu {
            menu.removed(self);
            self.inventory_menu
                .lock()
                .behavior_mut()
                .transfer_state(menu.behavior());
        }
        *open_menu = None;
    }

    /// Returns true if the player has an external menu open (not the inventory).
    #[must_use]
    pub fn has_container_open(&self) -> bool {
        self.open_menu.lock().is_some()
    }

    /// Broadcasts inventory changes to the client (incremental sync).
    /// This is called every tick to sync only changed slots.
    pub fn broadcast_inventory_changes(&self) {
        let mut open_menu = self.open_menu.lock();
        if let Some(ref mut menu) = *open_menu {
            menu.behavior_mut().broadcast_changes(&self.connection);
        } else {
            drop(open_menu);
            self.inventory_menu
                .lock()
                .behavior_mut()
                .broadcast_changes(&self.connection);
        }
    }

    /// Drops an item from the player's selected hotbar slot.
    ///
    /// Based on Java's `ServerPlayer.drop(boolean all)`.
    ///
    /// - `all`: If true, drops the entire stack (Ctrl+Q). If false, drops one item (Q).
    pub fn drop_from_selected(&self, all: bool) {
        if !self.can_drop_items() {
            return;
        }

        let removed = {
            let mut inventory = self.inventory.lock();
            let selected = inventory.get_selected_item_mut();
            if selected.is_empty() {
                return;
            }
            if all {
                selected.split(selected.count())
            } else {
                selected.split(1)
            }
        };

        self.drop_item(removed, false, true);
    }

    /// Drops an item into the world.
    ///
    /// Based on Java's `LivingEntity.drop(ItemStack, boolean randomly, boolean thrownFromHand)`.
    ///
    /// - `throw_randomly`: If true, the item is thrown in a random direction.
    ///   If false, it's thrown in the direction the player is facing.
    /// - `thrown_from_hand`: If true, sets the thrower and uses a longer pickup delay.
    pub fn drop_item(&self, item: ItemStack, throw_randomly: bool, thrown_from_hand: bool) {
        if item.is_empty() {
            return;
        }

        let pos = self.position();
        let (yaw, pitch) = self.rotation();

        let spawn_y = self.get_eye_y() - 0.3;

        let velocity = if throw_randomly {
            let power = rand::random::<f32>() * 0.5;
            let angle = rand::random::<f32>() * TAU;
            DVec3::new(
                f64::from(-angle.sin() * power),
                0.2,
                f64::from(angle.cos() * power),
            )
        } else {
            let pitch_rad = pitch.to_radians();
            let yaw_rad = yaw.to_radians();

            let sin_pitch = pitch_rad.sin();
            let cos_pitch = pitch_rad.cos();
            let sin_yaw = yaw_rad.sin();
            let cos_yaw = yaw_rad.cos();

            let angle_offset = rand::random::<f32>() * TAU;
            let power_offset = 0.02 * rand::random::<f32>();

            DVec3::new(
                f64::from(-sin_yaw * cos_pitch * 0.3)
                    + f64::from(angle_offset.cos() * power_offset),
                f64::from(-sin_pitch * 0.3 + 0.1)
                    + f64::from((rand::random::<f32>() - rand::random::<f32>()) * 0.1),
                f64::from(cos_yaw * cos_pitch * 0.3) + f64::from(angle_offset.sin() * power_offset),
            )
        };

        let spawn_pos = DVec3::new(pos.x, spawn_y, pos.z);

        if let Some(entity) = self
            .get_world()
            .spawn_item_with_velocity(spawn_pos, item, velocity)
        {
            entity.set_pickup_delay(40);
            if thrown_from_hand {
                entity.set_thrower(self.gameprofile.id);
            }
        }
    }

    /// Returns true if the player can drop items.
    ///
    /// Based on Java's `Player.canDropItems()`.
    /// Returns false if the player is dead, removed, or has a flag preventing item drops.
    #[must_use]
    pub fn can_drop_items(&self) -> bool {
        !self.is_removed()
        // TODO: Check if player is alive (health > 0)
    }

    /// Tries to add an item to the player's inventory, dropping it if it doesn't fit.
    ///
    /// Based on Java's `Inventory.placeItemBackInInventory`.
    pub fn add_item_or_drop(&self, mut item: ItemStack) {
        if item.is_empty() {
            return;
        }

        let added = self.inventory.lock().add(&mut item);
        if !added || !item.is_empty() {
            self.drop_item(item, false, false);
        }
    }

    /// Tries to add an item to the player's inventory using an existing lock guard,
    /// dropping it if it doesn't fit.
    ///
    /// Use this variant when you already hold a `ContainerLockGuard` that includes
    /// the player's inventory to avoid deadlocks.
    pub fn add_item_or_drop_with_guard(&self, guard: &mut ContainerLockGuard, mut item: ItemStack) {
        if item.is_empty() {
            return;
        }

        let inv_id = ContainerId::from_arc(&self.inventory);
        if let Some(inv) = guard.get_mut(inv_id) {
            let added = inv.add(&mut item);
            if !added || !item.is_empty() {
                self.drop_item(item, false, false);
            }
        } else {
            // Inventory not in guard - this shouldn't happen but drop the item to be safe
            self.drop_item(item, false, false);
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
    const fn mark_main_hand_dirty(&mut self) {
        self.dirty_main_hand = true;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_items::ITEMS;
    use steel_utils::Identifier;

    use super::*;

    #[test]
    fn add_marks_changed_when_stack_fills_existing_slot() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        inventory.items[0] = ItemStack::with_count(&ITEMS.oak_log, 63);
        let before = inventory.get_times_changed();

        let mut stack = ItemStack::new(&ITEMS.oak_log);
        assert!(inventory.add(&mut stack));

        assert!(stack.is_empty());
        assert_eq!(inventory.items[0].count(), 64);
        assert_ne!(inventory.get_times_changed(), before);
    }

    #[test]
    fn add_to_selected_existing_slot_marks_main_hand_dirty() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        inventory.items[0] = ItemStack::with_count(&ITEMS.oak_log, 63);
        inventory.drain_dirty_equipment_items();

        let mut stack = ItemStack::new(&ITEMS.oak_log);
        assert!(inventory.add(&mut stack));

        assert_eq!(
            inventory.drain_dirty_equipment_items(),
            vec![(
                EquipmentSlot::MainHand,
                ItemStack::with_count(&ITEMS.oak_log, 64)
            )]
        );
    }

    #[test]
    fn add_to_empty_selected_slot_marks_main_hand_dirty() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        inventory.drain_dirty_equipment_items();

        let mut stack = ItemStack::with_count(&ITEMS.oak_log, 3);
        assert!(inventory.add(&mut stack));

        assert_eq!(
            inventory.drain_dirty_equipment_items(),
            vec![(
                EquipmentSlot::MainHand,
                ItemStack::with_count(&ITEMS.oak_log, 3)
            )]
        );
    }

    #[test]
    fn clear_content_counts_equipment_items() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        inventory.items[0] = ItemStack::with_count(&ITEMS.oak_log, 3);
        inventory
            .equipment
            .set(EquipmentSlot::Head, ItemStack::new(&ITEMS.diamond_helmet));

        assert_eq!(inventory.clear_content(), 4);
        assert!(inventory.is_empty());
    }

    #[test]
    fn non_empty_equipment_items_uses_selected_item_as_main_hand() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        let main_hand = ItemStack::with_count(&ITEMS.oak_log, 2);
        let head = ItemStack::new(&ITEMS.diamond_helmet);
        inventory.items[0] = main_hand.clone();
        inventory
            .equipment
            .set(EquipmentSlot::MainHand, ItemStack::new(&ITEMS.stick));
        inventory.equipment.set(EquipmentSlot::Head, head.clone());

        let items = inventory.non_empty_equipment_items();

        assert_eq!(items.len(), 2);
        assert!(items.contains(&(EquipmentSlot::MainHand, main_hand)));
        assert!(items.contains(&(EquipmentSlot::Head, head)));
    }

    #[test]
    fn selected_slot_change_drains_main_hand_equipment_update_once() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        let selected = ItemStack::new(&ITEMS.oak_log);
        inventory.items[1] = selected.clone();

        inventory.set_selected_slot(1);
        let dirty_items = inventory.drain_dirty_equipment_items();

        assert_eq!(dirty_items, vec![(EquipmentSlot::MainHand, selected)]);
        assert!(inventory.drain_dirty_equipment_items().is_empty());
    }

    #[test]
    fn packet_selected_slot_rejects_invalid_values_without_wrapping() {
        let mut inventory = PlayerInventory::new(Weak::new());

        assert!(inventory.try_set_selected_slot_from_packet(8).is_ok());
        assert_eq!(inventory.get_selected_slot(), 8);

        assert_eq!(
            inventory.try_set_selected_slot_from_packet(9),
            Err(InvalidHotbarSlot)
        );
        assert_eq!(inventory.get_selected_slot(), 8);

        assert_eq!(
            inventory.try_set_selected_slot_from_packet(-1),
            Err(InvalidHotbarSlot)
        );
        assert_eq!(inventory.get_selected_slot(), 8);

        assert_eq!(
            inventory.try_set_selected_slot_from_packet(256),
            Err(InvalidHotbarSlot)
        );
        assert_eq!(inventory.get_selected_slot(), 8);
    }

    #[test]
    fn shrink_item_in_hand_marks_changed_and_dirty_equipment() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        inventory.set_selected_item(ItemStack::with_count(&ITEMS.oak_log, 3));
        inventory.set_offhand_item(ItemStack::with_count(&ITEMS.shield, 2));
        inventory.drain_dirty_equipment_items();

        let before = inventory.get_times_changed();
        inventory.shrink_item_in_hand(InteractionHand::MainHand, 1);

        assert_eq!(inventory.get_selected_item().count(), 2);
        assert_ne!(inventory.get_times_changed(), before);
        assert_eq!(
            inventory.drain_dirty_equipment_items(),
            vec![(
                EquipmentSlot::MainHand,
                ItemStack::with_count(&ITEMS.oak_log, 2)
            )]
        );

        let before = inventory.get_times_changed();
        inventory.shrink_item_in_hand(InteractionHand::OffHand, 1);

        assert_eq!(inventory.get_offhand_item().count(), 1);
        assert_ne!(inventory.get_times_changed(), before);
        assert_eq!(
            inventory.drain_dirty_equipment_items(),
            vec![(
                EquipmentSlot::OffHand,
                ItemStack::with_count(&ITEMS.shield, 1)
            )]
        );
    }

    #[test]
    fn split_item_in_hand_marks_changed_and_dirty_equipment() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        inventory.set_selected_item(ItemStack::with_count(&ITEMS.oak_log, 3));
        inventory.drain_dirty_equipment_items();

        let before = inventory.get_times_changed();
        let split = inventory.split_item_in_hand(InteractionHand::MainHand, 1);

        assert_eq!(split, ItemStack::with_count(&ITEMS.oak_log, 1));
        assert_eq!(inventory.get_selected_item().count(), 2);
        assert_ne!(inventory.get_times_changed(), before);
        assert_eq!(
            inventory.drain_dirty_equipment_items(),
            vec![(
                EquipmentSlot::MainHand,
                ItemStack::with_count(&ITEMS.oak_log, 2)
            )]
        );
    }

    #[test]
    fn hurt_item_in_hand_marks_changed_and_dirty_equipment() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        inventory.set_selected_item(ItemStack::new(&ITEMS.shears));
        inventory.drain_dirty_equipment_items();

        let before = inventory.get_times_changed();
        inventory.hurt_item_in_hand(InteractionHand::MainHand, 1, false);

        let main_hand = inventory.get_selected_item();
        assert!(main_hand.is(&ITEMS.shears));
        assert_eq!(main_hand.get_damage_value(), 1);
        let expected = main_hand.copy_with_count(1);
        assert_ne!(inventory.get_times_changed(), before);
        assert_eq!(
            inventory.drain_dirty_equipment_items(),
            vec![(EquipmentSlot::MainHand, expected)]
        );
    }

    #[test]
    fn hurt_and_convert_item_in_hand_damages_without_breaking() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        inventory.set_offhand_item(ItemStack::new(&ITEMS.carrot_on_a_stick));
        inventory.drain_dirty_equipment_items();

        let before = inventory.get_times_changed();
        inventory.hurt_and_convert_item_in_hand_on_break(
            InteractionHand::OffHand,
            1,
            &ITEMS.fishing_rod,
            false,
        );

        let offhand = inventory.get_offhand_item();
        assert!(offhand.is(&ITEMS.carrot_on_a_stick));
        assert_eq!(offhand.get_damage_value(), 1);
        let expected = offhand.copy_with_count(1);
        assert_ne!(inventory.get_times_changed(), before);
        assert_eq!(
            inventory.drain_dirty_equipment_items(),
            vec![(EquipmentSlot::OffHand, expected)]
        );
    }

    #[test]
    fn hurt_and_convert_item_in_hand_replaces_broken_item() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        inventory.set_selected_item(ItemStack::new(&ITEMS.carrot_on_a_stick));
        let max_damage = inventory.get_selected_item().get_max_damage();
        inventory
            .get_selected_item_mut()
            .set_damage_value(max_damage - 1);
        inventory.drain_dirty_equipment_items();

        let before = inventory.get_times_changed();
        inventory.hurt_and_convert_item_in_hand_on_break(
            InteractionHand::MainHand,
            7,
            &ITEMS.fishing_rod,
            false,
        );

        let main_hand = inventory.get_selected_item();
        assert!(main_hand.is(&ITEMS.fishing_rod));
        assert_eq!(main_hand.count(), 1);
        assert_eq!(main_hand.get_damage_value(), 0);
        let expected = main_hand.copy_with_count(1);
        assert_ne!(inventory.get_times_changed(), before);
        assert_eq!(
            inventory.drain_dirty_equipment_items(),
            vec![(EquipmentSlot::MainHand, expected)]
        );
    }

    #[test]
    fn swap_hands_swaps_selected_and_offhand() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        let main_hand = ItemStack::with_count(&ITEMS.oak_log, 3);
        let offhand = ItemStack::new(&ITEMS.shield);
        inventory.set_selected_item(main_hand.clone());
        inventory.set_offhand_item(offhand.clone());
        inventory.drain_dirty_equipment_items();

        assert!(inventory.swap_hands());

        assert_eq!(inventory.get_selected_item(), &offhand);
        assert_eq!(inventory.get_offhand_item(), &main_hand);
        let dirty_items = inventory.drain_dirty_equipment_items();
        assert!(dirty_items.contains(&(EquipmentSlot::MainHand, offhand)));
        assert!(dirty_items.contains(&(EquipmentSlot::OffHand, main_hand)));
    }

    #[test]
    fn equippable_single_item_moves_to_empty_armor_slot() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        inventory.set_selected_item(ItemStack::new(&ITEMS.diamond_helmet));

        let result = inventory.try_swap_with_equipment_slot(
            InteractionHand::MainHand,
            EquipmentSlot::Head,
            false,
        );

        assert_eq!(result, EquipmentSwapResult::Success(ItemStack::empty()));
        assert!(inventory.get_selected_item().is_empty());
        assert_eq!(
            inventory.equipment().get_ref(EquipmentSlot::Head),
            &ItemStack::new(&ITEMS.diamond_helmet)
        );
    }

    #[test]
    fn equippable_swap_respects_prevent_armor_change_effect() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        let mut bound_helmet = ItemStack::new(&ITEMS.diamond_helmet);
        bound_helmet.set_enchantments(&[(Identifier::vanilla_static("binding_curse"), 1)], false);
        inventory.set_selected_item(ItemStack::new(&ITEMS.carved_pumpkin));
        inventory
            .equipment_mut()
            .set(EquipmentSlot::Head, bound_helmet.copy_with_count(1));

        let result = inventory.try_swap_with_equipment_slot(
            InteractionHand::MainHand,
            EquipmentSlot::Head,
            false,
        );

        assert_eq!(result, EquipmentSwapResult::Fail);
        assert_eq!(
            inventory.get_selected_item(),
            &ItemStack::new(&ITEMS.carved_pumpkin)
        );
        assert_eq!(
            inventory.equipment().get_ref(EquipmentSlot::Head),
            &bound_helmet
        );
    }

    #[test]
    fn repair_with_xp_repairs_damaged_mending_item() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        let mut pickaxe = ItemStack::new(&ITEMS.diamond_pickaxe);
        pickaxe.set_damage_value(10);
        pickaxe.set_enchantments(&[(Identifier::vanilla_static("mending"), 1)], false);
        inventory.set_selected_item(pickaxe);
        inventory.drain_dirty_equipment_items();
        let before = inventory.get_times_changed();

        let remaining = inventory.repair_random_equipped_item_with_xp(3);

        assert_eq!(remaining, 0);
        assert_eq!(inventory.get_selected_item().get_damage_value(), 4);
        assert_ne!(inventory.get_times_changed(), before);
        assert_eq!(
            inventory.drain_dirty_equipment_items(),
            vec![(
                EquipmentSlot::MainHand,
                inventory.get_selected_item().copy_with_count(1)
            )]
        );
    }

    #[test]
    fn repair_with_xp_returns_leftover_when_item_is_fully_repaired() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        let mut pickaxe = ItemStack::new(&ITEMS.diamond_pickaxe);
        pickaxe.set_damage_value(3);
        pickaxe.set_enchantments(&[(Identifier::vanilla_static("mending"), 1)], false);
        inventory.set_selected_item(pickaxe);

        let remaining = inventory.repair_random_equipped_item_with_xp(5);

        assert_eq!(remaining, 4);
        assert_eq!(inventory.get_selected_item().get_damage_value(), 0);
    }

    #[test]
    fn equippable_stack_moves_one_item_and_returns_old_equipment_to_inventory() {
        init_test_registry();

        let mut inventory = PlayerInventory::new(Weak::new());
        inventory.set_selected_item(ItemStack::with_count(&ITEMS.carved_pumpkin, 2));
        inventory
            .equipment_mut()
            .set(EquipmentSlot::Head, ItemStack::new(&ITEMS.diamond_helmet));

        let result = inventory.try_swap_with_equipment_slot(
            InteractionHand::MainHand,
            EquipmentSlot::Head,
            false,
        );

        assert_eq!(result, EquipmentSwapResult::Success(ItemStack::empty()));
        assert_eq!(inventory.get_selected_item().count(), 1);
        assert_eq!(
            inventory.equipment().get_ref(EquipmentSlot::Head),
            &ItemStack::new(&ITEMS.carved_pumpkin)
        );
        assert!(
            inventory
                .get_items()
                .iter()
                .any(|stack| stack.is(&ITEMS.diamond_helmet))
        );
    }
}
