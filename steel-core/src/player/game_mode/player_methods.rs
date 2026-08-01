use super::{
    AttributeModifier, AttributeModifierOperation, Axis, BlockCollisionContext, BlockPos,
    CBlockChangedAck, CChangeDifficulty, CGameEvent, CPlayerInfoUpdate,
    CREATIVE_BLOCK_RANGE_MODIFIER_AMOUNT, CREATIVE_ENTITY_RANGE_MODIFIER_AMOUNT, CSetCamera,
    CollisionWorld, Difficulty, Entity, FLIGHT_DISABLE_RANGE, GameEventType, GameType, Identifier,
    LivingEntity, Player, SSpectatorAction, WorldAabb, WorldCollisionProvider,
    player_can_change_difficulty, shapes, vanilla_attributes,
};

impl Player {
    /// Sets the player's game mode and notifies the client.
    ///
    /// Returns `true` if the game mode was changed, `false` if the player was already in the requested game mode.
    pub fn set_game_mode(&self, gamemode: GameType) -> bool {
        let was_spectator = self.game_mode() == GameType::Spectator;
        if !self.change_game_mode_state(gamemode) {
            return false;
        }

        // Update abilities based on new game mode (mirrors vanilla GameType.updatePlayerAbilities)
        let flying_after_update = {
            let mut abilities = self.abilities.lock();
            abilities.update_for_game_mode(gamemode);
            abilities.flying
        };
        if flying_after_update
            && gamemode != GameType::Spectator
            && self.is_in_range_of_ground_for_flight_disable()
        {
            self.set_flying(false);
        }
        self.send_abilities();
        self.update_game_mode_invisibility();

        let update_packet =
            CPlayerInfoUpdate::update_game_mode(self.gameprofile.id, gamemode as i32);
        self.server().broadcast_to_online(update_packet);

        self.get_world().update_sleeping_player_list();

        if gamemode == GameType::Creative {
            self.reset_current_impulse_context();
        }

        self.send_packet(CGameEvent {
            event: GameEventType::ChangeGameMode,
            data: gamemode.into(),
        });

        if gamemode == GameType::Spectator {
            self.stop_riding();
            // TODO: Remove shoulder entities once player shoulder storage is implemented.
            // TODO: Stop item use once living item-use state is implemented.
            // TODO: Stop location-based enchantment effects once those effects are implemented.
        } else if was_spectator {
            self.send_packet(CSetCamera {
                camera_id: self.id(),
            });
            // TODO: Restart location-based enchantment effects once those effects are implemented.
        }

        self.send_abilities();
        self.update_game_mode_invisibility();
        self.living_base.mark_effects_dirty();

        true
    }

    fn update_game_mode_invisibility(&self) {
        self.living_base.mark_effects_dirty();
        self.update_dirty_mob_effect_entity_data();
        self.sync_entity_data();
    }

    fn is_in_range_of_ground_for_flight_disable(&self) -> bool {
        let world = self.get_world();
        let collision_world = WorldCollisionProvider::for_entity(&world, self);
        let bounding_box = self.bounding_box();
        let collision_context =
            BlockCollisionContext::entity(self.position().y, self.is_descending())
                .with_fall_distance(self.fall_distance())
                .with_can_walk_on_powder_snow(self.can_walk_on_powder_snow());

        if collision_world.has_collision_with_context(&bounding_box, collision_context) {
            return false;
        }

        let below = WorldAabb::new(
            bounding_box.min_x(),
            bounding_box.min_y() - FLIGHT_DISABLE_RANGE,
            bounding_box.min_z(),
            bounding_box.max_x(),
            bounding_box.min_y(),
            bounding_box.max_z(),
        );
        let colliders = collision_world.get_collisions_with_context(&below, collision_context);
        if colliders.is_empty() {
            return false;
        }

        let available_space_below =
            -shapes::collide(Axis::Y, &bounding_box, &colliders, -FLIGHT_DISABLE_RANGE);
        available_space_below < FLIGHT_DISABLE_RANGE
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
        let world = self.get_world();
        if !player_can_change_difficulty(self, &world) {
            log::warn!(
                "Player {} tried to change difficulty to {difficulty:?} without permission",
                self.gameprofile.name
            );
            return;
        }
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
        for world in self.server().worlds.worlds_in_domain(&domain) {
            world.set_difficulty(difficulty);
        }
    }

    /// Updates interaction range attribute modifiers based on game mode.
    ///
    /// Vanilla: `ServerPlayer.updatePlayerAttributes()` — applies creative-mode
    /// range modifiers every tick.
    pub(in crate::player) fn update_player_attributes(&self) {
        let is_creative = self.game_mode() == GameType::Creative;
        let mut attrs = self.attributes().lock();

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
        self.game_mode() == GameType::Creative
    }

    /// Acknowledges block changes up to the given sequence number.
    ///
    /// The ack is batched and sent once per tick (in `tick_ack_block_changes`),
    /// matching vanilla behavior.
    pub fn ack_block_changes_up_to(&self, sequence: i32) {
        self.tick_state.lock().ack_block_changes_up_to(sequence);
    }

    /// Sends pending block change ack if any. Called once per tick.
    pub(in crate::player) fn tick_ack_block_changes(&self) {
        let sequence = self.tick_state.lock().take_ack_block_changes_up_to();
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
        self.is_within_block_interaction_range_with_buffer(pos, 1.0)
    }

    /// Returns true if player is within block interaction range plus a vanilla buffer.
    #[must_use]
    pub fn is_within_block_interaction_range_with_buffer(
        &self,
        pos: BlockPos,
        buffer: f64,
    ) -> bool {
        let player_pos = self.position();
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
            .attributes()
            .lock()
            .get_value(vanilla_attributes::BLOCK_INTERACTION_RANGE)
            .unwrap_or(4.5);
        let max_range = base_range + buffer;
        dist_sq < max_range * max_range
    }

    /// Returns true if the player's eye position is within entity interaction range.
    #[must_use]
    pub fn is_within_entity_interaction_range(&self, aabb: WorldAabb, buffer: f64) -> bool {
        let player_pos = self.position();
        let eye_y = player_pos.y + self.get_eye_height();

        let dx = f64::max(
            f64::max(aabb.min_x() - player_pos.x, player_pos.x - aabb.max_x()),
            0.0,
        );
        let dy = f64::max(f64::max(aabb.min_y() - eye_y, eye_y - aabb.max_y()), 0.0);
        let dz = f64::max(
            f64::max(aabb.min_z() - player_pos.z, player_pos.z - aabb.max_z()),
            0.0,
        );
        let dist_sq = dx * dx + dy * dy + dz * dz;

        let base_range = self
            .attributes()
            .lock()
            .get_value(vanilla_attributes::ENTITY_INTERACTION_RANGE)
            .unwrap_or(3.0);
        let max_range = base_range + buffer;
        dist_sq < max_range * max_range
    }

    /// Handles spectator camera selection.
    pub fn handle_spectator_action(&self, packet: SSpectatorAction) {
        if !self.has_client_loaded() || self.game_mode() != GameType::Spectator {
            return;
        }

        let Some(entity_id) = packet.spectate_entity_id else {
            return;
        };

        let world = self.get_world();
        let Some(target) = world.get_accessible_entity_by_id(entity_id) else {
            return;
        };

        // TODO: Store the camera entity and apply world-border/cross-world
        // setCamera semantics once those foundations exist.
        if self.is_within_entity_interaction_range(target.bounding_box(), 3.0)
            && target.is_pickable()
        {
            self.send_packet(CSetCamera {
                camera_id: target.id(),
            });
        }
    }

    /// Returns true if player is sneaking (secondary use active).
    #[must_use]
    pub fn is_secondary_use_active(&self) -> bool {
        self.is_crouching()
    }
}
