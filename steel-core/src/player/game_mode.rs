//! Game mode specific logic for player interactions.
//!
//! This module implements the logic from Java's `ServerPlayerGameMode`, particularly
//! the `useItemOn` method that handles block placement and block interactions.

use std::sync::{Arc, atomic::Ordering};

use steel_protocol::packets::game::{
    AnimateAction, CAnimate, CBlockChangedAck, CBlockUpdate, CChangeDifficulty, CGameEvent,
    COpenSignEditor, CPlayerInfoUpdate, CSetHeldSlot, GameEventType, PlayerAction,
    SPickItemFromBlock, SPlayerAction, SSignUpdate, SUseItem, SUseItemOn,
};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::{REGISTRY, vanilla_attributes};
use steel_utils::BlockPos;
use steel_utils::Identifier;
use steel_utils::translations;
use steel_utils::types::{Difficulty, GameType, InteractionHand};
use text_components::TextComponent;

use crate::behavior::{
    BLOCK_BEHAVIORS, BlockHitResult, ITEM_BEHAVIORS, InteractionResult, UseOnContext,
};
use crate::block_entity::BlockEntity;
use crate::block_entity::entities::SignBlockEntity;
use crate::command::commands::gamemode::get_gamemode_translation;
use crate::entity::Entity;
use crate::entity::attribute::{AttributeModifier, AttributeModifierOperation};
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::Menu;
use crate::player::Player;
use crate::player::block_breaking::BlockBreakAction;
use crate::player::player_inventory::PlayerInventory;
use crate::world::World;

const CREATIVE_BLOCK_RANGE_MODIFIER_AMOUNT: f64 = 0.5;
const CREATIVE_ENTITY_RANGE_MODIFIER_AMOUNT: f64 = 2.0;

/// Handles using an item on a block.
///
/// This implements the logic from Java's `ServerPlayerGameMode.useItemOn()`.
///
/// # Flow
/// 1. Spectator mode: Only allow opening menus (currently returns Pass)
/// 2. Check if block interaction should be suppressed (sneaking + holding items)
/// 3. If not suppressed: Call block's `use_item_on` method
/// 4. If block returns `TryEmptyHandInteraction` and main hand: Call block's `use_without_item`
/// 5. If item not empty: Call item behavior's `use_on` for placement
/// 6. Handle creative mode infinite materials
pub fn use_item_on(
    player: &Player,
    world: &Arc<World>,
    hand: InteractionHand,
    hit_result: &BlockHitResult,
) -> InteractionResult {
    let pos = hit_result.block_pos;
    let state = world.get_block_state(pos);

    // Spectator mode: can only open menus
    // TODO: Implement menu providers for blocks like chests
    if player.game_mode.load() == GameType::Spectator {
        return InteractionResult::Pass;
    }

    // Check if block interaction should be suppressed (sneaking + holding items in either hand)
    let have_something = {
        let inv = player.inventory.lock();
        !inv.get_item_in_hand(InteractionHand::MainHand).is_empty()
            || !inv.get_item_in_hand(InteractionHand::OffHand).is_empty()
    };

    let suppress_block_use = player.is_secondary_use_active() && have_something;

    // Get behavior registries
    let block_behaviors = &*BLOCK_BEHAVIORS;
    let item_behaviors = &*ITEM_BEHAVIORS;

    // Try block interaction first (if not suppressed).
    // No inventory lock held — block behaviors may need inventory access (e.g. opening chests).
    if !suppress_block_use {
        let Some(block) = REGISTRY.blocks.by_state_id(state) else {
            return InteractionResult::Pass;
        };
        let behavior = block_behaviors.get_behavior(block);

        // Brief lock for an immutable snapshot used during block interaction check
        let item_snapshot = player.inventory.lock().get_item_in_hand(hand).clone();

        let block_result =
            behavior.use_item_on(&item_snapshot, state, world, pos, player, hand, hit_result);

        if block_result.consumes_action() {
            return block_result;
        }

        if matches!(block_result, InteractionResult::TryEmptyHandInteraction)
            && hand == InteractionHand::MainHand
        {
            let empty_result = behavior.use_without_item(state, world, pos, player, hit_result);

            if empty_result.consumes_action() {
                return empty_result;
            }
        }
    }

    // Item use (block placement, etc.) — acquire inventory lock via ContainerLockGuard
    let inv_ref = ContainerRef::PlayerInventory(player.inventory.clone());
    let mut guard = ContainerLockGuard::lock_all(&[&inv_ref]);

    let inv_id = inv_ref.container_id();

    let is_empty = {
        let Some(inv) = guard.get_player_inventory_mut(inv_id) else {
            return InteractionResult::Pass;
        };
        inv.get_item_in_hand(hand).is_empty()
    };

    if !is_empty {
        // TODO: Check item cooldowns
        // if player.getCooldowns().isOnCooldown(item_stack.item) { return Pass }

        let mut context =
            UseOnContext::new(player, hand, hit_result.clone(), world, &mut guard, inv_id);

        let original_count = context.inv.item().count;
        let item_ref = context.inv.item().item;
        let item_behavior = item_behaviors.get_behavior(item_ref);
        let result = item_behavior.use_on(&mut context);

        // Restore count for creative mode (infinite materials)
        if player.has_infinite_materials() && context.inv.item().count < original_count {
            context.inv.item().count = original_count;
        }

        return result;
    }

    InteractionResult::Pass
}

/// Handles using an item (general usage like right-clicking air).
///
/// This implements logic similar to `ServerPlayerGameMode.useItem()`.
pub fn use_item(player: &Player, world: &Arc<World>, hand: InteractionHand) -> InteractionResult {
    // Spectator mode: can only open menus
    if player.game_mode.load() == GameType::Spectator {
        return InteractionResult::Pass;
    }

    // TODO: Check item cooldowns
    // if player.getCooldowns().isOnCooldown(item_stack) { return InteractionResult::Pass }

    let inv_ref = ContainerRef::PlayerInventory(player.inventory.clone());
    let mut guard = ContainerLockGuard::lock_all(&[&inv_ref]);
    let inv_id = inv_ref.container_id();

    let is_empty = {
        let Some(inv) = guard.get_player_inventory_mut(inv_id) else {
            return InteractionResult::Pass;
        };
        inv.get_item_in_hand(hand).is_empty()
    };

    if !is_empty {
        let mut context =
            crate::behavior::UseItemContext::new(player, hand, world, &mut guard, inv_id);

        let original_count = context.inv.item().count;

        // Get behavior registries
        let item_behaviors = &*ITEM_BEHAVIORS;
        let item_ref = context.inv.item().item;
        let item_behavior = item_behaviors.get_behavior(item_ref);

        let result = item_behavior.use_item(&mut context);

        // Restore count for creative mode (infinite materials)
        if player.has_infinite_materials() && context.inv.item().count < original_count {
            context.inv.item().count = original_count;
        }

        return result;
    }

    InteractionResult::Pass
}

impl Player {
    /// Sets the player's game mode and notifies the client.
    ///
    /// Returns `true` if the game mode was changed, `false` if the player was already in the requested game mode.
    pub fn set_game_mode(&self, gamemode: GameType) -> bool {
        let current_gamemode = self.game_mode.load();
        if current_gamemode == gamemode {
            return false;
        }

        self.prev_game_mode.store(self.game_mode.load());
        self.game_mode.store(gamemode);

        // Update abilities based on new game mode (mirrors vanilla GameType.updatePlayerAbilities)
        self.abilities.lock().update_for_game_mode(gamemode);
        self.send_abilities();

        self.send_packet(CGameEvent {
            event: GameEventType::ChangeGameMode,
            data: gamemode.into(),
        });

        let update_packet =
            CPlayerInfoUpdate::update_game_mode(self.gameprofile.id, gamemode as i32);
        self.get_world().broadcast_to_all(update_packet);

        self.send_message(
            &translations::COMMANDS_GAMEMODE_SUCCESS_SELF
                .message([get_gamemode_translation(gamemode)])
                .into(),
        );

        true
    }

    /// Sends the current world difficulty to the client.
    pub fn send_difficulty(&self) {
        let world = self.get_world();
        let level_data = world.level_data.read();
        let difficulty = level_data.data().difficulty;
        let locked = level_data.data().difficulty_locked;
        drop(level_data);
        self.send_packet(CChangeDifficulty { difficulty, locked });
    }

    /// Handles a client request to change the world difficulty.
    pub fn handle_change_difficulty(&self, difficulty: Difficulty) {
        // TODO: implement op-level permission check
        let world = self.get_world();
        {
            let level_data = world.level_data.read();
            if level_data.data().difficulty_locked {
                let current = level_data.data().difficulty;
                drop(level_data);
                self.send_packet(CChangeDifficulty {
                    difficulty: current,
                    locked: true,
                });
                return;
            }
        }

        let domain = self.get_world().domain().to_owned();
        for w in self.server().worlds.worlds_in_domain(&domain) {
            let mut level_data = w.level_data.write();
            level_data.data_mut().difficulty = difficulty;
            let locked = level_data.data().difficulty_locked;
            drop(level_data);

            w.broadcast_to_all(CChangeDifficulty { difficulty, locked });
        }
    }

    /// Updates interaction range attribute modifiers based on game mode.
    ///
    /// Vanilla: `ServerPlayer.updatePlayerAttributes()` — applies creative-mode
    /// range modifiers every tick.
    pub(super) fn update_player_attributes(&self) {
        let is_creative = self.game_mode.load() == GameType::Creative;
        let mut attrs = self.attributes.lock();

        if is_creative {
            attrs.set_modifier(
                vanilla_attributes::BLOCK_INTERACTION_RANGE,
                AttributeModifier {
                    id: Identifier::vanilla_static("creative_mode_block_range"),
                    amount: CREATIVE_BLOCK_RANGE_MODIFIER_AMOUNT,
                    operation: AttributeModifierOperation::AddValue,
                },
                false,
            );
            attrs.set_modifier(
                vanilla_attributes::ENTITY_INTERACTION_RANGE,
                AttributeModifier {
                    id: Identifier::vanilla_static("creative_mode_entity_range"),
                    amount: CREATIVE_ENTITY_RANGE_MODIFIER_AMOUNT,
                    operation: AttributeModifierOperation::AddValue,
                },
                false,
            );
        } else {
            attrs.remove_modifier(
                vanilla_attributes::BLOCK_INTERACTION_RANGE,
                &Identifier::vanilla_static("creative_mode_block_range"),
            );
            attrs.remove_modifier(
                vanilla_attributes::ENTITY_INTERACTION_RANGE,
                &Identifier::vanilla_static("creative_mode_entity_range"),
            );
        }
    }

    /// Returns true if player has infinite materials (Creative mode).
    #[must_use]
    pub fn has_infinite_materials(&self) -> bool {
        self.game_mode.load() == GameType::Creative
    }

    /// Acknowledges block changes up to the given sequence number.
    ///
    /// The ack is batched and sent once per tick (in `tick_ack_block_changes`),
    /// matching vanilla behavior.
    pub fn ack_block_changes_up_to(&self, sequence: i32) {
        let current = self.ack_block_changes_up_to.load(Ordering::Relaxed);
        if sequence > current {
            self.ack_block_changes_up_to
                .store(sequence, Ordering::Relaxed);
        }
    }

    /// Sends pending block change ack if any. Called once per tick.
    pub(super) fn tick_ack_block_changes(&self) {
        let sequence = self.ack_block_changes_up_to.swap(-1, Ordering::Relaxed);
        if sequence > -1 {
            self.send_packet(CBlockChangedAck { sequence });
        }
    }

    /// Returns true if player is within block interaction range.
    ///
    /// Uses eye position and AABB distance (nearest point on block surface),
    /// matching vanilla's `Player.isWithinBlockInteractionRange(pos, 1.0)`.
    #[must_use]
    pub fn is_within_block_interaction_range(&self, pos: BlockPos) -> bool {
        let player_pos = *self.position.lock();
        let eye_y = player_pos.y + self.get_eye_height();

        let min_x = f64::from(pos.x());
        let min_y = f64::from(pos.y());
        let min_z = f64::from(pos.z());
        let max_x = min_x + 1.0;
        let max_y = min_y + 1.0;
        let max_z = min_z + 1.0;

        let dx = f64::max(f64::max(min_x - player_pos.x, player_pos.x - max_x), 0.0);
        let dy = f64::max(f64::max(min_y - eye_y, eye_y - max_y), 0.0);
        let dz = f64::max(f64::max(min_z - player_pos.z, player_pos.z - max_z), 0.0);
        let dist_sq = dx * dx + dy * dy + dz * dz;

        let base_range = self
            .attributes
            .lock()
            .get_value(vanilla_attributes::BLOCK_INTERACTION_RANGE)
            .unwrap_or(4.5);
        let max_range = base_range + 1.0;
        dist_sq < max_range * max_range
    }

    /// Returns true if player is sneaking (secondary use active).
    #[must_use]
    pub fn is_secondary_use_active(&self) -> bool {
        self.entity_state.lock().crouching
    }

    /// Sends block update packets for a position and its neighbor.
    /// Optionally also sends an update for an additional placement position
    /// (useful for items like buckets that place blocks at different positions).
    fn send_block_updates(&self, pos: BlockPos, direction: Direction) {
        let world = self.get_world();
        let state = world.get_block_state(pos);
        self.send_packet(CBlockUpdate {
            pos,
            block_state: state,
        });

        let neighbor_pos = direction.relative(pos);
        let neighbor_state = world.get_block_state(neighbor_pos);
        self.send_packet(CBlockUpdate {
            pos: neighbor_pos,
            block_state: neighbor_state,
        });
    }

    /// Triggers arm swing animation and broadcasts it to nearby players.
    pub fn swing(&self, hand: InteractionHand, update_self: bool) {
        let action = match hand {
            InteractionHand::MainHand => AnimateAction::SwingMainHand,
            InteractionHand::OffHand => AnimateAction::SwingOffHand,
        };
        let packet = CAnimate::new(self.id, action);

        let chunk = *self.last_chunk_pos.lock();
        let exclude = if update_self { None } else { Some(self.id) };
        self.get_world().broadcast_to_nearby(chunk, packet, exclude);
    }

    /// Handles the use of an item on a block.
    ///
    /// Implements the logic from Java's `ServerGamePacketListenerImpl.handleUseItemOn()`.
    pub fn handle_use_item_on(&self, packet: SUseItemOn) {
        if !self.client_loaded.load(Ordering::Relaxed) {
            return;
        }

        self.ack_block_changes_up_to(packet.sequence);

        let pos = packet.block_hit.block_pos;
        let direction = packet.block_hit.direction;

        if !self.is_within_block_interaction_range(pos) {
            self.send_block_updates(pos, direction);
            return;
        }

        let center_x = f64::from(pos.x()) + 0.5;
        let center_y = f64::from(pos.y()) + 0.5;
        let center_z = f64::from(pos.z()) + 0.5;
        let location = &packet.block_hit.location;
        let limit = 1.000_000_1;

        if (location.x - center_x).abs() >= limit
            || (location.y - center_y).abs() >= limit
            || (location.z - center_z).abs() >= limit
        {
            log::warn!(
                "Rejecting UseItemOnPacket from {}: location {:?} too far from block {:?}",
                self.gameprofile.name,
                location,
                pos
            );
            self.send_block_updates(pos, direction);
            return;
        }

        let world = self.get_world();

        if pos.y() >= world.max_build_height() {
            // TODO: Send "build.tooHigh" message to player
            self.send_block_updates(pos, direction);
            return;
        }

        if self.is_awaiting_teleport() {
            self.send_block_updates(pos, direction);
            return;
        }

        if !world.may_interact(self, pos) {
            self.send_block_updates(pos, direction);
            return;
        }

        let result = use_item_on(self, &world, packet.hand, &packet.block_hit);

        if let InteractionResult::Success = result {
            // TODO: Trigger arm swing animation if needed
            self.swing(packet.hand, true);
        }

        self.send_block_updates(pos, direction);
        self.broadcast_inventory_changes();
    }

    /// Handles a player action packet (block breaking, item dropping, etc.).
    pub fn handle_player_action(&self, packet: SPlayerAction) {
        let world = self.get_world();
        match packet.action {
            PlayerAction::StartDestroyBlock => {
                self.block_breaking.lock().handle_block_break_action(
                    self,
                    &world,
                    packet.pos,
                    BlockBreakAction::Start,
                    packet.direction,
                );
                self.ack_block_changes_up_to(packet.sequence);
            }
            PlayerAction::StopDestroyBlock => {
                self.block_breaking.lock().handle_block_break_action(
                    self,
                    &world,
                    packet.pos,
                    BlockBreakAction::Stop,
                    packet.direction,
                );
                self.ack_block_changes_up_to(packet.sequence);
            }
            PlayerAction::AbortDestroyBlock => {
                self.block_breaking.lock().handle_block_break_action(
                    self,
                    &world,
                    packet.pos,
                    BlockBreakAction::Abort,
                    packet.direction,
                );
                self.ack_block_changes_up_to(packet.sequence);
            }
            PlayerAction::DropAllItems => {
                self.drop_from_selected(true);
            }
            PlayerAction::DropItem => {
                self.drop_from_selected(false);
            }
            PlayerAction::ReleaseUseItem => {
                // TODO: Implement release use item (releasing bow, etc.)
                log::debug!("Player {} released use item", self.gameprofile.name);
            }
            PlayerAction::SwapItemWithOffhand => {
                // TODO: Implement swap item with offhand (F key)
                log::debug!("Player {} wants to swap items", self.gameprofile.name);
            }
            PlayerAction::Stab => {
                log::debug!("Player {} performed stab action", self.gameprofile.name);
            }
        }
    }

    /// Handles the use of an item.
    pub fn handle_use_item(&self, packet: SUseItem) {
        log::info!(
            "Player {} used {:?} (sequence: {}, yaw: {}, pitch: {})",
            self.gameprofile.name,
            packet.hand,
            packet.sequence,
            packet.y_rot,
            packet.x_rot
        );

        self.ack_block_changes_up_to(packet.sequence);

        let world = self.get_world();
        let result = use_item(self, &world, packet.hand);

        if let InteractionResult::Success = result {
            self.swing(packet.hand, true);
        }

        self.broadcast_inventory_changes();
    }

    /// Handles the pick block action (middle click on a block).
    ///
    /// # Panics
    ///
    /// Panics if the behavior registry has not been initialized.
    pub fn handle_pick_item_from_block(&self, packet: SPickItemFromBlock) {
        if !self.is_within_block_interaction_range(packet.pos) {
            return;
        }

        let state = self.get_world().get_block_state(packet.pos);
        if state.is_air() {
            return;
        }

        let block = state.get_block();
        let block_behaviors = &*BLOCK_BEHAVIORS;
        let behavior = block_behaviors.get_behavior(block);

        let include_data = self.has_infinite_materials() && packet.include_data;

        let Some(item_stack) = behavior.get_clone_item_stack(block, state, include_data) else {
            return;
        };

        if item_stack.is_empty() {
            return;
        }

        // TODO: If include_data, add block entity NBT data to the item stack
        // This requires block entity support which isn't implemented yet

        let mut inventory = self.inventory.lock();

        let slot_with_item = inventory.find_slot_matching_item(&item_stack);

        if slot_with_item != -1 {
            if PlayerInventory::is_hotbar_slot(slot_with_item as usize) {
                inventory.set_selected_slot(slot_with_item as u8);
            } else {
                inventory.pick_slot(slot_with_item);
            }
        } else if self.has_infinite_materials() {
            inventory.add_and_pick_item(item_stack);
        } else {
            return;
        }

        self.send_packet(CSetHeldSlot {
            slot: i32::from(inventory.get_selected_slot()),
        });

        drop(inventory);
        self.inventory_menu
            .lock()
            .behavior_mut()
            .broadcast_changes(&self.connection);
    }

    /// Handles a sign update packet from the client.
    pub fn handle_sign_update(&self, packet: SSignUpdate) {
        if !self.is_within_block_interaction_range(packet.pos) {
            return;
        }

        let world = self.get_world();

        let Some(block_entity) = world.get_block_entity(packet.pos) else {
            return;
        };

        let mut guard = block_entity.lock();
        let Some(sign) = guard.as_any_mut().downcast_mut::<SignBlockEntity>() else {
            return;
        };

        if sign.is_waxed {
            return;
        }

        if sign.get_player_who_may_edit() != Some(self.gameprofile.id) {
            log::warn!(
                "Player {} tried to edit sign they're not allowed to edit",
                self.gameprofile.name
            );
            return;
        }

        let text = sign.get_text_mut(packet.is_front_text);
        for (i, line) in packet.lines.iter().enumerate() {
            if i < 4 {
                let stripped = strip_formatting_codes(line);
                text.set_message(i, TextComponent::plain(stripped));
            }
        }

        sign.set_player_who_may_edit(None);
        sign.set_changed();

        let update_tag = sign.get_update_tag();
        let block_entity_type = sign.get_type();
        let pos = packet.pos;

        drop(guard);

        if let Some(nbt) = update_tag {
            world.broadcast_block_entity_update(pos, block_entity_type, nbt);
        }
    }

    /// Opens the sign editor for the player.
    ///
    /// # Arguments
    /// * `pos` - Position of the sign block
    /// * `is_front_text` - Whether to edit front (true) or back (false) text
    pub fn open_sign_editor(&self, pos: BlockPos, is_front_text: bool) {
        let world = self.get_world();

        if let Some(block_entity) = world.get_block_entity(pos) {
            let mut guard = block_entity.lock();
            if let Some(sign) = guard.as_any_mut().downcast_mut::<SignBlockEntity>() {
                sign.set_player_who_may_edit(Some(self.gameprofile.id));
            }
        }

        let state = world.get_block_state(pos);
        self.send_packet(CBlockUpdate {
            pos,
            block_state: state,
        });

        self.send_packet(COpenSignEditor { pos, is_front_text });
    }
}

/// Strips Minecraft formatting codes (§ followed by a character) from a string.
///
/// This is equivalent to vanilla's `ChatFormatting.stripFormatting()`.
fn strip_formatting_codes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            result.push(c);
        }
    }

    result
}
