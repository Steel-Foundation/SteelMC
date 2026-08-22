use super::{block_breaking::BlockBreakAction, *};

impl Player {
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

    /// Triggers arm swing animation and broadcasts it to tracking players.
    pub fn swing(&self, hand: InteractionHand, update_self: bool) {
        LivingEntity::swing(self, hand, update_self);
    }

    /// Handles the use of an item on a block.
    ///
    /// Implements the logic from Java's `ServerGamePacketListenerImpl.handleUseItemOn()`.
    pub fn handle_use_item_on(&self, packet: SUseItemOn) {
        if !self.has_client_loaded() {
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
        let location_is_valid = (location.x - center_x).abs() < limit
            && (location.y - center_y).abs() < limit
            && (location.z - center_z).abs() < limit;

        if !location_is_valid {
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

        if result.should_swing_server() {
            self.swing(packet.hand, true);
        }

        self.send_block_updates(pos, direction);
        self.broadcast_inventory_changes();
    }

    /// Handles a player action packet (block breaking, item dropping, etc.).
    pub fn handle_player_action(&self, packet: SPlayerAction) {
        if !self.has_client_loaded() {
            return;
        }

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
                self.release_using_item();
            }
            PlayerAction::SwapItemWithOffhand => {
                if self.game_mode() == GameType::Spectator {
                    return;
                }

                let changed = self.inventory.lock().swap_hands();
                self.stop_using_item();
                if changed {
                    self.broadcast_inventory_changes();
                }
            }
            PlayerAction::Stab => {
                if self.game_mode() == GameType::Spectator {
                    return;
                }

                let main_hand_item = {
                    let inventory = self.inventory.lock();
                    let stack = inventory.get_item_in_hand(InteractionHand::MainHand);
                    stack.copy_with_count(stack.count())
                };
                if self.cannot_attack_with_item(&main_hand_item, 5) {
                    return;
                }
                if let Some(piercing_weapon) = main_hand_item.get_piercing_weapon() {
                    self.piercing_attack(&main_hand_item, piercing_weapon);
                }
            }
        }
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

        // TODO: If include_data, copy the block entity data into the picked item stack.

        let mut inventory = self.inventory.lock();

        let slot_with_item = inventory.find_slot_matching_item_with_same_components(&item_stack);

        if slot_with_item != -1 {
            if PlayerInventory::is_hotbar_slot(slot_with_item as usize) {
                inventory.set_selected_slot(slot_with_item as u8);
            } else {
                let slot = inventory.get_suitable_hotbar_slot();

                inventory.set_selected_slot(slot);
                inventory.pick_slot(slot_with_item);
            }
        } else if self.has_infinite_materials() {
            let slot = inventory.get_suitable_hotbar_slot();

            inventory.set_selected_slot(slot);
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

        let Some(sign) = block_entity.downcast_ref::<SignBlockEntity>() else {
            return;
        };

        if sign.is_waxed() {
            return;
        }

        if sign.get_player_who_may_edit() != Some(self.gameprofile.id) {
            log::warn!(
                "Player {} tried to edit sign they're not allowed to edit",
                self.gameprofile.name
            );
            return;
        }

        let mut text = sign.get_text(packet.is_front_text);
        for (i, line) in packet.lines.iter().enumerate() {
            if i < 4 {
                let stripped = strip_formatting_codes(line);
                text.set_message(i, TextComponent::plain(stripped));
            }
        }

        sign.set_text(text, packet.is_front_text);
        sign.set_player_who_may_edit(None);
        sign.set_changed();

        let update_tag = sign.get_update_tag();
        let block_entity_type = sign.get_type();
        let pos = packet.pos;

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

        if let Some(block_entity) = world.get_block_entity(pos)
            && let Some(sign) = block_entity.downcast_ref::<SignBlockEntity>()
        {
            sign.set_player_who_may_edit(Some(self.gameprofile.id));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::init_behaviors;
    use crate::player::connection::NetworkConnection as _;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
    use steel_registry::vanilla_items;
    use steel_utils::ChunkPos;

    #[test]
    fn use_item_on_rejects_non_finite_hit_locations() {
        let world = fresh_test_world("use_item_on_non_finite_hit_location");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        init_behaviors();
        let player = TestPlayerBuilder::new(world, "TestPlayer", 1).build();
        player.set_client_loaded(true);
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::FIREWORK_ROCKET));

        for (sequence, location) in [
            (1, DVec3::new(f64::NAN, 0.5, 0.5)),
            (2, DVec3::new(0.5, f64::INFINITY, 0.5)),
            (3, DVec3::new(0.5, 0.5, f64::NEG_INFINITY)),
        ] {
            player.handle_use_item_on(SUseItemOn {
                hand: InteractionHand::MainHand,
                block_hit: BlockHitResult {
                    location,
                    direction: Direction::Up,
                    block_pos: BlockPos::ZERO,
                    miss: false,
                    inside: false,
                    world_border_hit: false,
                },
                sequence,
            });
        }

        let inventory = player.inventory.lock();
        let held_item = inventory.get_item_in_hand(InteractionHand::MainHand);
        assert!(held_item.is(&vanilla_items::FIREWORK_ROCKET));
        assert_eq!(held_item.count(), 1);
        assert!(!player.connection.closed());
    }
}
