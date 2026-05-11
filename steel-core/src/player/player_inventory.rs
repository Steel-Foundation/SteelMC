//! Player inventory management.

use std::{
    array,
    f32::consts::TAU,
    sync::{LazyLock, Weak, atomic::Ordering},
};

use glam::DVec3;
use steel_protocol::packets::game::{
    CContainerClose, COpenScreen, SContainerButtonClick, SContainerClick, SContainerClose,
    SContainerSlotStateChanged, SSetCarriedItem, SSetCreativeModeSlot,
};
use steel_registry::item_stack::ItemStack;
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
    #[expect(
        dead_code,
        reason = "held for future use; player reference needed for inventory change notifications"
    )]
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

impl Player {
    /// Attempts to pick up nearby item entities.
    ///
    /// Mirrors vanilla's `Player.aiStep()` item pickup logic:
    /// - Calculates pickup area as bounding box inflated by (1.0, 0.5, 1.0)
    /// - Calls `playerTouch()` on each entity in range
    pub(super) fn touch_nearby_items(&self) {
        if self.game_mode.load() == GameType::Spectator {
            return;
        }

        let pickup_area = self.bounding_box().inflate_xyz(1.0, 0.5, 1.0);
        let world = self.get_world();
        let entities = world.get_entities_in_aabb(&pickup_area);

        let Some(player_arc) = world.players.get_by_entity_id(self.id) else {
            return;
        };

        for entity in entities {
            if entity.id() == self.id || entity.is_removed() {
                continue;
            }

            if let Some(item_entity) = entity.as_item_entity() {
                item_entity.try_pickup(&player_arc);
            }

            // TODO: Handle other entity types (experience orbs, arrows)
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
        if self.game_mode.load() == GameType::Spectator {
            menu.behavior_mut()
                .send_all_data_to_remote(&self.connection);
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

        let has_infinite_materials = self.game_mode.load() == GameType::Creative;
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
        if self.game_mode.load() != GameType::Creative {
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
                    slot.set_item(&mut guard, item_stack.clone());
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
        self.inventory.lock().set_selected_slot(packet.slot as u8);
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
        let current = self.container_counter.load(Ordering::Relaxed);
        let next = (current % 100) + 1;
        self.container_counter.store(next, Ordering::Relaxed);
        next
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
        let (yaw, pitch) = self.rotation.load();

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
            && thrown_from_hand
        {
            entity.set_pickup_delay(40);
            entity.set_thrower(self.gameprofile.id);
        }
    }

    /// Returns true if the player can drop items.
    ///
    /// Based on Java's `Player.canDropItems()`.
    /// Returns false if the player is dead, removed, or has a flag preventing item drops.
    #[must_use]
    pub fn can_drop_items(&self) -> bool {
        !self.removed.load(Ordering::Relaxed)
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
                self.items[slot] = stack.split(to_place);
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
