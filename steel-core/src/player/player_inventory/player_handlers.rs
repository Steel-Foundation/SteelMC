use std::{f32::consts::TAU, sync::Arc};

use glam::DVec3;
use steel_protocol::packets::game::{
    CContainerClose, COpenScreen, SContainerButtonClick, SContainerClick, SContainerClose,
    SContainerSlotStateChanged, SSetCarriedItem, SSetCreativeModeSlot,
};
use steel_registry::item_stack::ItemStack;
use steel_utils::types::GameType;

use crate::{
    entity::{Entity, entities::ItemEntity},
    inventory::{
        MenuProvider,
        container::{Container, clear_or_count_matching_stack},
        inventory_menu::InventoryMenu,
        lock::{ContainerId, ContainerLockGuard},
        menu::Menu,
        slot::Slot,
    },
    player::Player,
};

impl Player {
    /// Attempts to pick up nearby item entities.
    ///
    /// Mirrors vanilla's `Player.aiStep()` item pickup logic:
    /// - Calculates pickup area as bounding box inflated by (1.0, 0.5, 1.0)
    /// - Calls `playerTouch()` on each entity in range
    pub(in crate::player) fn touch_nearby_items(&self) {
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

    /// Removes or counts matching stacks across every location used by vanilla `/clear`.
    pub(crate) fn clear_or_count_matching_items(
        &self,
        predicate: &dyn Fn(&ItemStack) -> bool,
        amount_to_remove: i32,
    ) -> i32 {
        let counting_only = amount_to_remove == 0;
        let mut count = self.inventory.lock().clear_or_count_matching_items(
            predicate,
            amount_to_remove,
            counting_only,
        );

        {
            let inventory_menu = self.inventory_menu.lock();
            count += inventory_menu
                .crafting_container()
                .lock()
                .clear_or_count_matching_items(predicate, amount_to_remove - count, counting_only);
        }

        let has_open_menu = {
            let mut open_menu = self.open_menu.lock();
            if let Some(menu) = open_menu.as_mut() {
                let behavior = menu.behavior_mut();
                count += clear_or_count_matching_stack(
                    &mut behavior.carried,
                    predicate,
                    amount_to_remove - count,
                    counting_only,
                );
                if behavior.carried.is_empty() {
                    behavior.set_carried(ItemStack::empty());
                }
                true
            } else {
                false
            }
        };
        if !has_open_menu {
            let mut inventory_menu = self.inventory_menu.lock();
            let behavior = inventory_menu.behavior_mut();
            count += clear_or_count_matching_stack(
                &mut behavior.carried,
                predicate,
                amount_to_remove - count,
                counting_only,
            );
            if behavior.carried.is_empty() {
                behavior.set_carried(ItemStack::empty());
            }
        }

        self.inventory_menu.lock().update_crafting_result();
        self.broadcast_inventory_changes();
        count
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

        let _ = self.drop_item(removed, false, true);
    }

    /// Drops an item into the world.
    ///
    /// Based on Java's `LivingEntity.drop(ItemStack, boolean randomly, boolean thrownFromHand)`.
    ///
    /// - `throw_randomly`: If true, the item is thrown in a random direction.
    ///   If false, it's thrown in the direction the player is facing.
    /// - `thrown_from_hand`: If true, sets the thrower and uses a longer pickup delay.
    #[must_use]
    pub fn drop_item(
        &self,
        item: ItemStack,
        throw_randomly: bool,
        thrown_from_hand: bool,
    ) -> Option<Arc<ItemEntity>> {
        if item.is_empty() {
            return None;
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

        let entity = self
            .get_world()
            .spawn_item_with_velocity(spawn_pos, item, velocity)?;
        entity.set_pickup_delay(40);
        if thrown_from_hand {
            entity.set_thrower(self.gameprofile.id);
        }
        Some(entity)
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
            let _ = self.drop_item(item, false, false);
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
        let should_drop = if let Some(inv) = guard.get_mut(inv_id) {
            let added = inv.add(&mut item);
            !added || !item.is_empty()
        } else {
            true
        };
        if should_drop {
            let _ = guard.run_unlocked(|| self.drop_item(item, false, false));
        }
    }
}
