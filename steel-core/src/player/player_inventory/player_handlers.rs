use std::{f32::consts::TAU, mem, sync::Arc};

use crate::{
    entity::{Entity, LivingEntity as _, RemovalReason, entities::ItemEntity},
    inventory::{
        click::Click,
        container::{Container, CraftingContainer, clear_or_count_matching_stack},
        lock::{ContainerId, ContainerLockGuard},
        menu::{
            Menu,
            kinds::{INVENTORY_MENU_CONTAINER_ID, InventoryKind},
        },
        slots::CraftingHandler,
    },
    player::{Player, connection::NetworkConnection as _},
};
use glam::DVec3;
use steel_protocol::packets::game::{
    CContainerClose, COpenScreen, CSetPlayerInventory, ClickType, SContainerButtonClick,
    SContainerClick, SContainerClose, SContainerSlotStateChanged, SRenameItem, SSetCarriedItem,
    SSetCreativeModeSlot,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::vanilla_custom_stats;
use steel_utils::{
    Downcast as _,
    locks::Shared,
    types::{GameType, InteractionHand},
};
use text_components::TextComponent;

use super::{
    DeferredMenuAction, MenuItemDisposition, MenuOpenContext, MenuRemovalStatus, OpenMenuDispatch,
    OpenMenuUnavailable, PendingMenuOpen, PlayerInventory, PreparedMenu, TerminalMenuRemoval,
};

impl Player {
    fn take_open_menu_for_callback(
        &self,
        expected_container_id: Option<i32>,
    ) -> Result<Menu, OpenMenuUnavailable> {
        let mut open_menu = self.open_menu.lock();
        if open_menu.dispatch.is_some() {
            return Err(OpenMenuUnavailable::Unavailable);
        }

        let Some(menu) = open_menu.menu.as_ref() else {
            return Err(OpenMenuUnavailable::Closed);
        };
        if expected_container_id.is_some_and(|expected| i32::from(menu.container_id()) != expected)
        {
            return Err(OpenMenuUnavailable::Unavailable);
        }

        let container_id = menu.container_id();
        let overrides_player_slots = menu.overrides_player_slots();
        let Some(menu) = open_menu.menu.take() else {
            return Err(OpenMenuUnavailable::Unavailable);
        };
        open_menu.dispatch = Some(OpenMenuDispatch {
            container_id,
            overrides_player_slots,
            actions: Vec::new(),
        });
        Ok(menu)
    }

    fn finish_open_menu_callback(&self, menu: Menu) {
        let actions = {
            let mut open_menu = self.open_menu.lock();
            let Some(dispatch) = open_menu.dispatch.take() else {
                open_menu.menu = Some(menu);
                return;
            };
            if let Some(terminal_removal) = open_menu.terminal_removal.as_mut() {
                Self::queue_deferred_menus(terminal_removal, dispatch.actions);
                drop(open_menu);
                self.finish_terminal_menu_main_cleanup(Some(menu));
                return;
            }
            open_menu.menu = Some(menu);
            open_menu.active_open_operations += 1;
            dispatch.actions
        };

        self.run_deferred_menu_actions(actions);
    }

    fn finish_open_menu_removal(&self) {
        let actions = {
            let mut open_menu = self.open_menu.lock();
            let Some(dispatch) = open_menu.dispatch.take() else {
                return;
            };
            if let Some(terminal_removal) = open_menu.terminal_removal.as_mut() {
                Self::queue_deferred_menus(terminal_removal, dispatch.actions);
                drop(open_menu);
                self.finish_terminal_menu_main_cleanup(None);
                return;
            }
            open_menu.active_open_operations += 1;
            dispatch.actions
        };

        self.run_deferred_menu_actions(actions);
    }

    fn run_deferred_menu_actions(&self, actions: Vec<DeferredMenuAction>) {
        for action in actions {
            match action {
                DeferredMenuAction::Close { send_packet } => {
                    if send_packet {
                        self.close_container();
                    } else {
                        self.do_close_container();
                    }
                }
                DeferredMenuAction::Open(prepared) => {
                    self.execute_menu_open(*prepared);
                }
                DeferredMenuAction::Install(prepared) => {
                    let PreparedMenu { title, menu } = *prepared;
                    self.open_prepared_menu(title, menu);
                }
            }
        }

        self.finish_menu_open_operation();
    }

    fn queue_deferred_menus(
        terminal_removal: &mut TerminalMenuRemoval,
        actions: Vec<DeferredMenuAction>,
    ) {
        terminal_removal
            .pending_menus
            .extend(actions.into_iter().filter_map(|action| match action {
                DeferredMenuAction::Install(prepared) => Some(prepared.menu),
                DeferredMenuAction::Close { .. } | DeferredMenuAction::Open(_) => None,
            }));
    }

    fn begin_menu_open_operation(&self) -> bool {
        let mut open_menu = self.open_menu.lock();
        if open_menu.terminal_removal.is_some() {
            return false;
        }
        open_menu.active_open_operations += 1;
        true
    }

    fn finish_menu_open_operation(&self) {
        {
            let mut open_menu = self.open_menu.lock();
            debug_assert!(open_menu.active_open_operations > 0);
            open_menu.active_open_operations -= 1;
        }
        self.try_finish_terminal_menu_removal();
    }

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
        self.reset_last_action_time();
        match self.take_open_menu_for_callback(Some(packet.container_id)) {
            Ok(mut menu) => {
                self.process_container_click(&mut menu, packet);
                self.finish_open_menu_callback(menu);
            }
            Err(OpenMenuUnavailable::Closed) => {
                let mut menu = self.inventory_menu.lock();
                if i32::from(menu.behavior().container_id()) == packet.container_id {
                    self.process_container_click(&mut menu, packet);
                }
            }
            Err(OpenMenuUnavailable::Unavailable) => {}
        }
    }

    /// Processes a container click on any menu implementing the Menu trait.
    ///
    /// This is the common implementation shared between inventory menu and
    /// external menus (crafting table, chest, etc.).
    fn process_container_click(&self, menu: &mut Menu, packet: SContainerClick) {
        if self.game_mode() == GameType::Spectator || self.get_health() <= 0.0 {
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

        // Vanilla rejects positive out-of-range slots before applying client
        // prediction hashes or resynchronizing the menu. Its validity check
        // admits every negative slot because each is less than the slot count.
        let slot_count = menu.behavior().slot_count();
        let packet_slot_is_valid = packet.slot_num < 0
            || usize::try_from(packet.slot_num).is_ok_and(|slot| slot < slot_count);
        if !packet_slot_is_valid {
            log::debug!(
                "Player {} clicked invalid slot index: {}, available slots: {}",
                self.gameprofile.name,
                packet.slot_num,
                slot_count
            );
            return;
        }

        // Parse and validate the remaining raw click fields once. A malformed
        // button or drag encoding is not applied, but the state sync below
        // still runs so the client's prediction gets corrected.
        let click = Click::parse(
            packet.slot_num,
            packet.button_num,
            packet.click_type,
            slot_count,
        );
        if click.is_none() {
            log::debug!(
                "Player {} sent malformed container click (slot {}, button {}, {:?})",
                self.gameprofile.name,
                packet.slot_num,
                packet.button_num,
                packet.click_type
            );
            // Vanilla rejects positive out-of-range slots before `doClick`.
            // Once admitted, any non-QuickCraft input cancels an active drag,
            // and malformed QuickCraft headers/types reset their own state.
            let quick_craft_header = packet.button_num & 3;
            let quick_craft_type = (packet.button_num >> 2) & 3;
            if packet.click_type != ClickType::QuickCraft
                || quick_craft_header == 3
                || (quick_craft_header == 0 && quick_craft_type == 3)
            {
                menu.behavior_mut().reset_quick_craft();
            }
        }

        let full_resync_needed = packet.state_id as u32 != menu.behavior().state_id();

        menu.behavior_mut().suppress_remote_updates();

        if let Some(click) = click {
            menu.clicked(click, self);
        }

        for (slot, hash) in packet.changed_slots {
            let slot = slot as usize;
            // Result/fake slots are server-authoritative (their contents are
            // recomputed from a recipe). Don't let the client's prediction set
            // our view of what it knows, or `broadcast_changes` will think the
            // client already has the freshly-crafted result and skip syncing it
            // — leaving the slot blank until the next click forces a resend.
            if menu
                .behavior()
                .slots()
                .get(slot)
                .is_some_and(|slot| slot.is_fake())
            {
                menu.behavior_mut().mark_remote_slot_unknown(slot);
                continue;
            }
            menu.behavior_mut().set_remote_slot(slot, hash);
        }

        menu.behavior_mut().set_remote_carried(packet.carried_item);
        menu.behavior_mut().resume_remote_updates();

        if full_resync_needed {
            menu.behavior_mut()
                .send_all_data_to_remote(&self.connection);
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
        let closes_open_menu = open_menu
            .menu
            .as_ref()
            .is_some_and(|menu| i32::from(menu.container_id()) == packet.container_id)
            || open_menu
                .dispatch
                .as_ref()
                .is_some_and(|dispatch| i32::from(dispatch.container_id) == packet.container_id);
        drop(open_menu);

        if closes_open_menu {
            self.do_close_container();
            return;
        }

        if packet.container_id == i32::from(INVENTORY_MENU_CONTAINER_ID) {
            let mut menu = self.inventory_menu.lock();
            menu.removed(self);
        }
    }

    /// Handles an anvil rename packet.
    pub fn handle_rename_item(self: &Arc<Self>, packet: SRenameItem) {
        match self.take_open_menu_for_callback(None) {
            Ok(mut menu) => {
                if menu.still_valid(self) {
                    menu.set_item_name(packet.name, self);
                }
                self.finish_open_menu_callback(menu);
            }
            Err(OpenMenuUnavailable::Closed) => {
                log::debug!("rename item without an open menu");
            }
            Err(OpenMenuUnavailable::Unavailable) => {}
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
                if let Some(slot) = menu.behavior().slots().get(slot_index) {
                    let previous = slot.get_item(&guard).clone();
                    slot.set_by_player(&mut guard, item_stack.clone(), &previous);
                }
            }
            if (1..=4).contains(&slot_index) {
                menu.update_crafting_result();
            }
            menu.behavior_mut()
                .set_remote_slot_known(slot_index, &item_stack);
            menu.behavior_mut().broadcast_changes(&self.connection);
        } else if drop && valid_data {
            {
                let mut throttler = self.drop_spam_throttler.lock();
                if throttler.is_under_threshold() {
                    throttler.increment();
                } else {
                    log::warn!(
                        "Player {} was dropping items too fast in creative mode; ignoring",
                        self.gameprofile.name,
                    );
                    return;
                }
            }
            let _ = self.drop_item(item_stack, false, true);
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
        } else {
            self.reset_last_action_time();
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
    /// * `title` - The display title shown in the open-screen packet.
    /// * `create` - Factory invoked with the allocated container id, player,
    ///   and current world. If called by a menu hook, the factory runs after
    ///   that hook releases its container locks.
    ///
    /// # Panics
    /// Panics if the created menu uses a different container id than the one
    /// allocated for it, or has no menu type (i.e. the player's own inventory
    /// menu, which must never be opened via `open_menu`).
    pub fn open_menu(
        &self,
        title: impl Into<TextComponent>,
        create: impl for<'a> FnOnce(MenuOpenContext<'a>) -> Menu + Send + 'static,
    ) {
        if !self.begin_menu_open_operation() {
            return;
        }
        self.open_menu_inner(PendingMenuOpen {
            title: title.into(),
            create: Box::new(create),
        });
        self.finish_menu_open_operation();
    }

    fn open_menu_inner(&self, pending: PendingMenuOpen) {
        self.do_close_container();

        let mut open_menu = self.open_menu.lock();
        if open_menu.terminal_removal.is_some() {
            return;
        }
        if let Some(dispatch) = open_menu.dispatch.as_mut() {
            dispatch
                .actions
                .push(DeferredMenuAction::Open(Box::new(pending)));
            return;
        }
        drop(open_menu);

        self.execute_menu_open(pending);
    }

    fn execute_menu_open(&self, pending: PendingMenuOpen) {
        {
            let mut open_menu = self.open_menu.lock();
            if open_menu.terminal_removal.is_some() {
                return;
            }
            if let Some(dispatch) = open_menu.dispatch.as_mut() {
                dispatch
                    .actions
                    .push(DeferredMenuAction::Open(Box::new(pending)));
                return;
            }
        }

        let PendingMenuOpen { title, create } = pending;
        let container_id = self.next_container_counter();
        let world = self.get_world();
        let menu = create(MenuOpenContext {
            container_id,
            player: self,
            world: &world,
        });
        assert_eq!(
            menu.container_id(),
            container_id,
            "open_menu factory returned container id {}, but {} was allocated",
            menu.container_id(),
            container_id,
        );
        self.open_prepared_menu(title, menu);
    }

    fn open_prepared_menu(&self, title: TextComponent, mut menu: Menu) {
        loop {
            {
                let mut open_menu = self.open_menu.lock();
                if let Some(terminal_removal) = open_menu.terminal_removal.as_mut() {
                    terminal_removal.pending_menus.push(menu);
                    return;
                }
            }

            // A removal hook may have opened another menu while the initiating
            // open call was closing its predecessor.
            self.do_close_container();

            let mut open_menu = self.open_menu.lock();
            if let Some(terminal_removal) = open_menu.terminal_removal.as_mut() {
                terminal_removal.pending_menus.push(menu);
                return;
            }
            if let Some(dispatch) = open_menu.dispatch.as_mut() {
                dispatch
                    .actions
                    .push(DeferredMenuAction::Install(Box::new(PreparedMenu {
                        title,
                        menu,
                    })));
                return;
            }
            if open_menu.menu.is_some() {
                continue;
            }
            open_menu.dispatch = Some(OpenMenuDispatch {
                container_id: menu.container_id(),
                overrides_player_slots: menu.overrides_player_slots(),
                actions: Vec::new(),
            });
            break;
        }

        self.send_packet(COpenScreen {
            container_id: i32::from(menu.container_id()),
            menu_type: menu
                .menu_type()
                .expect("a menu opened via open_menu must declare a menu type"),
            title,
        });

        // Fire on_open before the full sync so anything the menu populates here
        // is included in the first render sent below.
        menu.on_open(self);

        menu.behavior_mut()
            .send_all_data_to_remote(&self.connection);

        self.finish_open_menu_callback(menu);
    }

    /// A shared handle to the 2x2 crafting grid of the always-open inventory
    /// menu.
    pub fn crafting_container(&self) -> Shared<CraftingContainer> {
        let menu = self.inventory_menu.lock();
        let Some(kind) = menu.kind().downcast_ref::<InventoryKind>() else {
            unreachable!("a player's inventory_menu is always the Inventory kind");
        };
        kind.crafting_container()
    }

    /// A shared handler for the 2x2 crafting grid of the always-open inventory
    /// menu and its result.
    pub(crate) fn inventory_crafting_handler(&self) -> CraftingHandler {
        let menu = self.inventory_menu.lock();
        let Some(kind) = menu.kind().downcast_ref::<InventoryKind>() else {
            unreachable!("a player's inventory_menu is always the Inventory kind");
        };
        kind.crafting_handler()
    }

    /// Closes the currently open container and returns to the inventory menu.
    ///
    /// Based on Java's `ServerPlayer::closeContainer`.
    /// This sends a close packet to the client.
    pub fn close_container(&self) {
        self.close_open_menu(true);
    }

    /// Internal close container logic without sending a packet.
    ///
    /// Based on Java's `ServerPlayer::doCloseContainer`.
    /// Called when the client sends a close packet or when opening a new menu.
    pub fn do_close_container(&self) {
        self.close_open_menu(false);
    }

    /// Removes both the base inventory menu and any external menu.
    ///
    /// This mirrors `Player::remove`: base crafting and carried items are
    /// handled before the external menu, and menu hooks cannot install a
    /// replacement while removal is in progress. The inventory menu remains
    /// reusable because Steel keeps one `Player` across world changes.
    pub fn remove_all_menus(&self) -> MenuRemovalStatus {
        self.remove_all_menus_with_disposition(self.default_menu_item_disposition())
    }

    pub(in crate::player) fn remove_all_menus_with_disposition(
        &self,
        disposition: MenuItemDisposition,
    ) -> MenuRemovalStatus {
        let menu = {
            let mut open_menu = self.open_menu.lock();
            if let Some(terminal_removal) = open_menu.terminal_removal.as_mut() {
                terminal_removal.disposition = terminal_removal.disposition.combine(disposition);
                return MenuRemovalStatus::Pending;
            }

            open_menu.terminal_removal = Some(TerminalMenuRemoval {
                disposition,
                main_cleanup_complete: false,
                pending_cleanup_in_progress: false,
                pending_menus: Vec::new(),
            });
            if open_menu.dispatch.is_some() {
                return MenuRemovalStatus::Pending;
            }

            open_menu.menu.take()
        };

        self.finish_terminal_menu_main_cleanup(menu);
        if self.open_menu.lock().terminal_removal.is_none() {
            MenuRemovalStatus::Complete
        } else {
            MenuRemovalStatus::Pending
        }
    }

    fn finish_terminal_menu_main_cleanup(&self, mut menu: Option<Menu>) {
        self.inventory_menu.lock().removed(self);
        if let Some(menu) = menu.as_mut() {
            self.remove_open_menu(menu);
        }

        {
            let mut open_menu = self.open_menu.lock();
            let Some(terminal_removal) = open_menu.terminal_removal.as_mut() else {
                return;
            };
            terminal_removal.main_cleanup_complete = true;
        }
        self.try_finish_terminal_menu_removal();
    }

    fn try_finish_terminal_menu_removal(&self) {
        loop {
            let pending_menus = {
                let mut open_menu = self.open_menu.lock();
                if open_menu.active_open_operations != 0 {
                    return;
                }
                let Some(terminal_removal) = open_menu.terminal_removal.as_mut() else {
                    return;
                };
                if !terminal_removal.main_cleanup_complete {
                    return;
                }
                if terminal_removal.pending_cleanup_in_progress {
                    return;
                }
                if terminal_removal.pending_menus.is_empty() {
                    open_menu.terminal_removal = None;
                    debug_assert!(open_menu.menu.is_none());
                    return;
                }
                terminal_removal.pending_cleanup_in_progress = true;
                mem::take(&mut terminal_removal.pending_menus)
            };

            for mut pending_menu in pending_menus {
                pending_menu.removed(self);
            }

            let mut open_menu = self.open_menu.lock();
            let Some(terminal_removal) = open_menu.terminal_removal.as_mut() else {
                return;
            };
            terminal_removal.pending_cleanup_in_progress = false;
        }
    }

    #[cfg(test)]
    pub(in crate::player) fn retry_terminal_menu_removal_for_test(&self) {
        self.try_finish_terminal_menu_removal();
    }

    fn close_open_menu(&self, send_packet: bool) {
        let menu = {
            let mut open_menu = self.open_menu.lock();
            if open_menu.terminal_removal.is_some() {
                return;
            }
            if let Some(dispatch) = open_menu.dispatch.as_mut() {
                dispatch
                    .actions
                    .push(DeferredMenuAction::Close { send_packet });
                return;
            }
            let Some(menu) = open_menu.menu.take() else {
                return;
            };
            open_menu.dispatch = Some(OpenMenuDispatch {
                container_id: menu.container_id(),
                overrides_player_slots: menu.overrides_player_slots(),
                actions: Vec::new(),
            });
            menu
        };

        let mut menu = menu;
        if send_packet {
            self.send_packet(CContainerClose {
                container_id: i32::from(menu.container_id()),
            });
        }
        self.remove_open_menu(&mut menu);
        self.finish_open_menu_removal();
    }

    fn remove_open_menu(&self, menu: &mut Menu) {
        let overrides_player_slots = menu.overrides_player_slots();
        menu.removed(self);
        if overrides_player_slots {
            self.request_inventory_resync(0..PlayerInventory::INVENTORY_SIZE);
        } else {
            self.inventory_menu
                .lock()
                .behavior_mut()
                .transfer_state(menu.behavior());
        }
    }

    /// Returns true if the player has an external menu open (not the inventory).
    #[must_use]
    pub fn has_container_open(&self) -> bool {
        let open_menu = self.open_menu.lock();
        open_menu.menu.is_some() || open_menu.dispatch.is_some()
    }

    /// Runs the open menu's per-tick hook, if an external menu is open.
    ///
    /// Scoped to the opened menu; the base inventory menu is not ticked. Called
    /// once per player tick, before syncing inventory changes to the client.
    pub fn tick_open_menu(&self) {
        let Ok(mut menu) = self.take_open_menu_for_callback(None) else {
            return;
        };
        if !menu.still_valid(self) {
            self.close_container();
            self.finish_open_menu_callback(menu);
            return;
        }
        menu.on_tick(self);
        self.finish_open_menu_callback(menu);
    }

    /// Broadcasts inventory changes to the client (incremental sync).
    /// This is called every tick to sync only changed slots.
    pub fn broadcast_inventory_changes(&self) {
        let mut open_menu = self.open_menu.lock();
        if let Some(menu) = open_menu.menu.as_mut() {
            menu.behavior_mut().broadcast_changes(&self.connection);
            return;
        }
        if open_menu.dispatch.is_none() {
            drop(open_menu);
            self.inventory_menu
                .lock()
                .behavior_mut()
                .broadcast_changes(&self.connection);
        }
    }

    /// Requests direct synchronization of logical player-inventory slots.
    pub(crate) fn request_inventory_resync(&self, slots: impl IntoIterator<Item = usize>) {
        self.inventory_sync.lock().request(slots);
    }

    /// Sends the latest values for requested logical inventory slots.
    pub(in crate::player) fn flush_inventory_resync(&self) {
        let overrides_player_slots = {
            let open_menu = self.open_menu.lock();
            open_menu
                .menu
                .as_ref()
                .is_some_and(Menu::overrides_player_slots)
                || open_menu
                    .dispatch
                    .as_ref()
                    .is_some_and(|dispatch| dispatch.overrides_player_slots)
        };
        let slots = self
            .inventory_sync
            .lock()
            .take_ready(overrides_player_slots);
        if slots.is_empty() {
            return;
        }

        let packets = {
            let inventory = self.inventory.lock();
            slots
                .into_iter()
                .map(|slot| CSetPlayerInventory {
                    slot: slot as i32,
                    item_stack: inventory.get_item(slot).clone(),
                })
                .collect::<Vec<_>>()
        };
        for packet in packets {
            self.send_packet(packet);
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

        count += self.inventory_menu.lock().clear_or_count_crafting_items(
            predicate,
            amount_to_remove - count,
            counting_only,
        );

        let has_open_menu = {
            let mut open_menu = self.open_menu.lock();
            if let Some(menu) = open_menu.menu.as_mut() {
                let behavior = menu.behavior_mut();
                count += clear_or_count_matching_stack(
                    behavior.carried_mut(),
                    predicate,
                    amount_to_remove - count,
                    counting_only,
                );
                if behavior.carried().is_empty() {
                    *behavior.carried_mut() = ItemStack::empty();
                }
                true
            } else {
                open_menu.dispatch.is_some()
            }
        };
        if !has_open_menu {
            let mut inventory_menu = self.inventory_menu.lock();
            let behavior = inventory_menu.behavior_mut();
            count += clear_or_count_matching_stack(
                behavior.carried_mut(),
                predicate,
                amount_to_remove - count,
                counting_only,
            );
            if behavior.carried().is_empty() {
                *behavior.carried_mut() = ItemStack::empty();
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
            let selected_count = inventory.get_selected_item().count();
            if selected_count == 0 {
                return;
            }
            inventory.split_item_in_hand(
                InteractionHand::MainHand,
                if all { selected_count } else { 1 },
            )
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

        let item_ref = item.item;
        let item_count = item.count;

        let entity = self
            .get_world()
            .spawn_item_with_velocity(spawn_pos, item, velocity)?;
        entity.set_pickup_delay(40);
        if thrown_from_hand {
            entity.set_thrower(self.gameprofile.id);
            self.award_stat_with_count(&vanilla_stat_types::ITEM_DROPPED, item_ref, item_count);
            self.award_custom_stat(&vanilla_custom_stats::DROP);
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

    /// Returns whether items from a closing menu (crafting grid, anvil inputs,
    /// cursor) should be placed back into the inventory instead of dropped into
    /// the world.
    ///
    /// Matches vanilla's `AbstractContainerMenu.dropOrPlaceInInventory`: a
    /// disconnected player or one removed for any reason except a world change
    /// drops the items.
    #[must_use]
    pub fn returns_menu_items_to_inventory(&self) -> bool {
        if let Some(disposition) = self
            .open_menu
            .lock()
            .terminal_removal
            .as_ref()
            .map(|terminal_removal| terminal_removal.disposition)
        {
            return disposition == MenuItemDisposition::ReturnToInventory;
        }

        self.default_menu_item_disposition() == MenuItemDisposition::ReturnToInventory
    }

    fn default_menu_item_disposition(&self) -> MenuItemDisposition {
        let removed_outside_world_change =
            self.is_removed() && self.removal_reason() != Some(RemovalReason::ChangedWorld);
        if removed_outside_world_change || self.connection.closed() || self.get_health() <= 0.0 {
            MenuItemDisposition::Drop
        } else {
            MenuItemDisposition::ReturnToInventory
        }
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
