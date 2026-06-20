//! Player movement physics and validation.
//!
//! This module handles server-side movement simulation and anti-cheat checks.
//! It implements collision detection and physics similar to vanilla Minecraft.

use glam::DVec3;
use steel_protocol::packets::game::{
    CMoveVehicle, CPlayerPosition, PlayerCommandAction, SAcceptTeleportation, SMovePlayer,
    SMoveVehicle, SPlayerCommand, SPlayerInput,
};
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_game_rules::{ELYTRA_MOVEMENT_CHECK, PLAYER_MOVEMENT_CHECK};
use steel_registry::vanilla_mob_effects;
use steel_utils::translations;
use steel_utils::types::GameType;

use crate::entity::{
    AcceptedClientMovement, AcceptedClientMovementOutcome, Entity, EntityMoveError, LivingEntity,
    get_input_vector,
};
use crate::physics::{
    MOVEMENT_ERROR_THRESHOLD, MovementCollisionValidation, MoverType, WorldCollisionProvider,
    is_colliding_with_new_shapes, movement_error_delta,
};
use crate::player::food_data::food_constants;
use crate::player::{Player, PlayerInput};
use crate::world::World;

/// Default gravity for players (blocks/tick²). Vanilla uses 0.08.
pub const DEFAULT_GRAVITY: f64 = 0.08;

/// Maximum movement speed threshold for normal movement (meters per tick squared).
pub const SPEED_THRESHOLD_NORMAL: f64 = 100.0;
/// Maximum movement speed threshold for elytra flight (meters per tick squared).
pub const SPEED_THRESHOLD_FLYING: f64 = 300.0;

/// Horizontal position clamping limit (matches vanilla).
pub const CLAMP_HORIZONTAL: f64 = 3.0E7;
/// Vertical position clamping limit (matches vanilla).
pub const CLAMP_VERTICAL: f64 = 2.0E7;

/// Clamps a horizontal coordinate to vanilla limits.
#[must_use]
pub fn clamp_horizontal(value: f64) -> f64 {
    value.clamp(-CLAMP_HORIZONTAL, CLAMP_HORIZONTAL)
}

/// Clamps a vertical coordinate to vanilla limits.
#[must_use]
pub fn clamp_vertical(value: f64) -> f64 {
    value.clamp(-CLAMP_VERTICAL, CLAMP_VERTICAL)
}

#[must_use]
fn wrap_degrees(mut degrees: f32) -> f32 {
    degrees %= 360.0;
    if degrees >= 180.0 {
        degrees -= 360.0;
    }
    if degrees < -180.0 {
        degrees += 360.0;
    }
    degrees
}

#[derive(Debug, Clone, Copy)]
struct PlayerFloatingValidation {
    y_dist: f64,
    player_stands_on_something: bool,
    is_spectator: bool,
    server_allows_flight: bool,
    may_fly: bool,
    has_levitation: bool,
    is_fall_flying: bool,
}

impl PlayerFloatingValidation {
    fn can_violate(self) -> bool {
        self.y_dist >= -0.03125
            && !self.player_stands_on_something
            && !self.is_spectator
            && !self.server_allows_flight
            && !self.may_fly
            && !self.has_levitation
            && !self.is_fall_flying
    }
}

impl Player {
    const fn is_invalid_position(x: f64, y: f64, z: f64, rot_x: f32, rot_y: f32) -> bool {
        if x.is_nan() || y.is_nan() || z.is_nan() {
            return true;
        }

        if !rot_x.is_finite() || !rot_y.is_finite() {
            return true;
        }

        false
    }

    fn move_vehicle_packet_from_entity(entity: &dyn Entity) -> CMoveVehicle {
        let rotation = entity.rotation();
        CMoveVehicle {
            position: entity.position(),
            y_rot: rotation.0,
            x_rot: rotation.1,
        }
    }

    /// Checks if we're awaiting a teleport confirmation and handles timeout/resend.
    ///
    /// Returns `true` if awaiting teleport (movement should be rejected),
    /// `false` if normal movement processing should continue.
    fn update_awaiting_teleport(&self) -> bool {
        let mut tp = self.teleport_state.lock();
        let Some(pos) = tp.awaiting_position else {
            tp.teleport_time = self.tick_count();
            return false;
        };

        let current_tick = self.tick_count();

        // Resend teleport after 20 ticks (~1 second) timeout
        if current_tick.wrapping_sub(tp.teleport_time) > 20 {
            drop(tp);

            let (yaw, pitch) = self.rotation();
            if let Err(error) = self.teleport(pos, yaw, pitch) {
                log::warn!(
                    "Failed to resend pending teleport for player {}: {error}",
                    self.id()
                );
            }
        }
        true
    }

    /// Applies vanilla post-impulse movement validation grace.
    pub fn apply_post_impulse_grace_time(&self, ticks: i32) {
        LivingEntity::apply_post_impulse_grace_time(self, ticks);
    }

    /// Resets per-tick vanilla movement validation bases for the controlled root vehicle.
    pub(super) fn reset_vehicle_movement_for_tick(&self) {
        let Some(vehicle) = self.root_vehicle() else {
            self.movement.lock().clear_vehicle_for_tick();
            return;
        };

        let controlled_by_player = vehicle
            .controlling_passenger()
            .is_some_and(|controller| controller.id() == self.id());
        if !controlled_by_player {
            self.movement.lock().clear_vehicle_for_tick();
            return;
        }

        self.movement
            .lock()
            .reset_vehicle_for_tick(vehicle.id(), vehicle.position());
    }

    /// Checks if movement validation should be performed for this player.
    ///
    /// Matches vanilla's `ServerGamePacketListenerImpl.shouldValidateMovement()`.
    /// Uses the `playerMovementCheck` and `elytraMovementCheck` gamerules.
    ///
    /// Returns `true` if movement should be validated, `false` to skip validation.
    fn should_validate_movement(world: &World, is_fall_flying: bool) -> bool {
        let player_check = world.get_game_rule(&PLAYER_MOVEMENT_CHECK);
        if player_check != GameRuleValue::Bool(true) {
            return false;
        }

        if is_fall_flying {
            let elytra_check = world.get_game_rule(&ELYTRA_MOVEMENT_CHECK);
            return elytra_check == GameRuleValue::Bool(true);
        }

        true
    }

    /// Handles a move player packet.
    ///
    /// Matches vanilla `ServerGamePacketListenerImpl.handleMovePlayer()`.
    ///
    /// # Panics
    ///
    /// Panics if the server cannot restore the player to the last accepted position after rejecting
    /// invalid movement. That indicates world entity state refused an authoritative correction.
    #[expect(
        clippy::too_many_lines,
        reason = "matches vanilla handleMovePlayer; splitting would hurt readability"
    )]
    pub fn handle_move_player(&self, packet: SMovePlayer) {
        if Self::is_invalid_position(
            packet.get_x(0.0),
            packet.get_y(0.0),
            packet.get_z(0.0),
            packet.get_x_rot(0.0),
            packet.get_y_rot(0.0),
        ) {
            self.disconnect(translations::MULTIPLAYER_DISCONNECT_INVALID_PLAYER_MOVEMENT.msg());
            return;
        }

        let current_rotation = self.rotation();
        let target_yaw = wrap_degrees(packet.get_y_rot(current_rotation.0));
        let target_pitch = wrap_degrees(packet.get_x_rot(current_rotation.1));

        if self.update_awaiting_teleport() {
            self.set_rotation((target_yaw, target_pitch));
            return;
        }

        if !self.has_client_loaded() {
            return;
        }

        let start_pos = self.position();
        let target_pos = DVec3::new(
            clamp_horizontal(packet.get_x(start_pos.x)),
            clamp_vertical(packet.get_y(start_pos.y)),
            clamp_horizontal(packet.get_z(start_pos.z)),
        );
        let game_mode = self.game_mode();
        let is_sleeping = self.is_sleeping();
        let is_fall_flying = self.is_fall_flying();
        let was_on_ground = self.on_ground();
        let is_spectator = game_mode == GameType::Spectator;
        let is_creative = game_mode == GameType::Creative;
        let world = self.get_world();
        let tick_runs_normally = world.tick_runs_normally();
        if self.is_passenger() {
            let passenger_pos = self.position();
            if let Err(error) = self.try_set_position(passenger_pos) {
                log::warn!(
                    "Failed to refresh passenger player {} position during movement: {error}",
                    self.id()
                );
                return;
            }
            self.set_rotation((target_yaw, target_pitch));
            world.chunk_map.update_player_status(self);
            return;
        }

        let (first_good, last_good) = self.movement.lock().good_positions();

        if is_sleeping {
            let dx = target_pos.x - first_good.x;
            let dy = target_pos.y - first_good.y;
            let dz = target_pos.z - first_good.z;
            let moved_dist_sq = dx * dx + dy * dy + dz * dz;

            if moved_dist_sq > 1.0 {
                if let Err(error) = self.teleport(start_pos, target_yaw, target_pitch) {
                    log::warn!(
                        "Failed to correct sleeping player {} movement: {error}",
                        self.id()
                    );
                }
                return;
            }
            return;
        }

        let dx = target_pos.x - first_good.x;
        let dy = target_pos.y - first_good.y;
        let dz = target_pos.z - first_good.z;
        let moved_dist_sq = dx * dx + dy * dy + dz * dz;

        if tick_runs_normally {
            let mut delta_packets = {
                let mut mv = self.movement.lock();
                mv.record_move_packet_delta()
            };

            if delta_packets > 5 {
                delta_packets = 1;
            }

            if Self::should_validate_movement(&world, is_fall_flying) {
                let threshold = if is_fall_flying {
                    SPEED_THRESHOLD_FLYING
                } else {
                    SPEED_THRESHOLD_NORMAL
                } * f64::from(delta_packets);

                if moved_dist_sq - self.velocity().length_squared() > threshold {
                    if let Err(error) =
                        self.teleport(start_pos, current_rotation.0, current_rotation.1)
                    {
                        log::warn!(
                            "Failed to correct too-fast player {} movement: {error}",
                            self.id()
                        );
                    }
                    return;
                }
            }
        }

        let old_aabb = self.bounding_box();
        let move_delta = target_pos - last_good;
        let moved_upwards = move_delta.y > 0.0;
        let player_stands_on_something = self.vertical_collision_below();

        if was_on_ground && !packet.on_ground && moved_upwards {
            self.jump_from_ground();
        }

        if self.move_entity(MoverType::Player, move_delta).is_none() {
            if let Err(error) = self.teleport(start_pos, target_yaw, target_pitch) {
                panic!(
                    "failed to correct rejected player {} movement: {error}",
                    self.id()
                );
            }
            return;
        }

        let error_delta = movement_error_delta(target_pos, self.position());
        let error_dist_sq = error_delta.length_squared();
        let in_impulse_grace = self.is_in_post_impulse_grace_time();
        let fail = error_dist_sq > MOVEMENT_ERROR_THRESHOLD
            && !is_creative
            && !is_spectator
            && !in_impulse_grace;

        let new_aabb = self.bounding_box().translate(target_pos - self.position());
        let collision_world = WorldCollisionProvider::for_entity(&world, self);
        let old_collision = collision_world.has_entity_context_collision(
            old_aabb,
            self.position().y,
            self.is_descending(),
        );
        let new_collision =
            is_colliding_with_new_shapes(&collision_world, old_aabb, new_aabb, self.is_crouching());

        if (MovementCollisionValidation {
            no_physics: self.no_physics(),
            moved_wrongly: fail,
            old_collision,
            new_collision,
        })
        .rejects()
        {
            if let Err(error) = self.teleport(start_pos, target_yaw, target_pitch) {
                log::warn!(
                    "Failed to correct collided player {} movement: {error}",
                    self.id()
                );
            }
            self.refresh_supporting_block_for_fall_damage(DVec3::ZERO, packet.on_ground);
            self.do_check_fall_damage(DVec3::ZERO, packet.on_ground, &world);
            self.remove_latest_movement_recording();
            return;
        }

        // Vanilla saves this requested Y delta before recomputing the
        // post-move residual used by moved-wrongly validation.
        let floating_check = Some((player_stands_on_something, move_delta.y));

        if packet.on_ground && self.is_sprinting() {
            let dx = move_delta.x;
            let dz = move_delta.z;

            let cm = ((dx * dx + dz * dz).sqrt() as f32 * 100.0).round() as i32;
            if cm > 0 {
                self.cause_food_exhaustion(food_constants::EXHAUSTION_SPRINT * cm as f32 * 0.01);
            }
        }

        let client_delta = target_pos - start_pos;
        match self.apply_accepted_client_movement(
            &world,
            AcceptedClientMovement {
                position: Some(target_pos),
                rotation: (target_yaw, target_pitch),
                on_ground: packet.on_ground,
                horizontal_collision: packet.horizontal_collision,
                movement: client_delta,
                reset_fall_distance: moved_upwards,
            },
        ) {
            Ok(AcceptedClientMovementOutcome::Applied) => {}
            Ok(AcceptedClientMovementOutcome::Handled) => return,
            Err(error) => {
                log::warn!(
                    "Rejected accepted player movement for entity {}: {error}",
                    self.id()
                );
                if let Err(teleport_error) = self.teleport(start_pos, target_yaw, target_pitch) {
                    log::warn!(
                        "Failed to correct rejected player movement for entity {}: {teleport_error}",
                        self.id()
                    );
                }
                self.remove_latest_movement_recording();
                return;
            }
        }
        world.chunk_map.update_player_status(self);

        if let Some((player_stands_on_something, y_dist)) = floating_check {
            self.record_client_floating(
                &world,
                y_dist,
                player_stands_on_something,
                is_spectator,
                is_fall_flying,
            );
        }
        self.movement
            .lock()
            .mark_last_good_position(self.position());

        self.movement
            .lock()
            .set_last_known_client_movement(client_delta);
    }

    /// Handles a controlled-vehicle movement packet.
    ///
    /// Matches vanilla `ServerGamePacketListenerImpl.handleMoveVehicle()`.
    #[expect(
        clippy::too_many_lines,
        reason = "matches vanilla handleMoveVehicle; splitting would hurt readability"
    )]
    pub fn handle_move_vehicle(&self, packet: SMoveVehicle) {
        if Self::is_invalid_position(
            packet.pos.x,
            packet.pos.y,
            packet.pos.z,
            packet.x_rot,
            packet.y_rot,
        ) {
            self.disconnect(translations::MULTIPLAYER_DISCONNECT_INVALID_VEHICLE_MOVEMENT.msg());
            return;
        }

        if self.update_awaiting_teleport() || !self.has_client_loaded() {
            return;
        }

        let Some(vehicle) = self.root_vehicle() else {
            return;
        };
        let controlled_by_player = vehicle
            .controlling_passenger()
            .is_some_and(|controller| controller.id() == self.id());
        if !controlled_by_player {
            return;
        }
        let Some((first_good, last_good)) =
            self.movement.lock().vehicle_good_positions(vehicle.id())
        else {
            return;
        };

        let world = self.get_world();
        let old_position = vehicle.position();
        let target_pos = DVec3::new(
            clamp_horizontal(packet.pos.x),
            clamp_vertical(packet.pos.y),
            clamp_horizontal(packet.pos.z),
        );
        let target_yaw = wrap_degrees(packet.y_rot);
        let target_pitch = wrap_degrees(packet.x_rot);
        let first_good_delta = target_pos - first_good;
        let moved_dist_sq = first_good_delta.length_squared();
        let expected_dist_sq = vehicle.velocity().length_squared();
        if moved_dist_sq - expected_dist_sq > SPEED_THRESHOLD_NORMAL {
            log::warn!(
                "{} (vehicle of {}) moved too quickly! {},{},{}",
                vehicle.id(),
                self.gameprofile.name,
                first_good_delta.x,
                first_good_delta.y,
                first_good_delta.z
            );
            self.send_packet(Self::move_vehicle_packet_from_entity(vehicle.as_ref()));
            return;
        }

        let old_aabb = vehicle.bounding_box();
        let move_delta = target_pos - last_good;
        let vehicle_rests_on_something = vehicle.vertical_collision_below();
        if vehicle.is_living_entity() && vehicle.on_climbable() {
            vehicle.reset_fall_distance();
        }

        if vehicle.move_entity(MoverType::Player, move_delta).is_none() {
            self.send_packet(Self::move_vehicle_packet_from_entity(vehicle.as_ref()));
            return;
        }

        let error_delta = movement_error_delta(target_pos, vehicle.position());
        let error_dist_sq = error_delta.length_squared();
        let fail = error_dist_sq > MOVEMENT_ERROR_THRESHOLD;
        if fail {
            log::warn!(
                "{} (vehicle of {}) moved wrongly! {}",
                vehicle.id(),
                self.gameprofile.name,
                error_dist_sq.sqrt()
            );
        }

        let new_aabb = vehicle
            .bounding_box()
            .translate(target_pos - vehicle.position());
        let collision_world = WorldCollisionProvider::for_entity(&world, vehicle.as_ref());
        let old_collision = collision_world.has_entity_context_collision(
            old_aabb,
            vehicle.position().y,
            vehicle.is_descending(),
        );
        let new_collision = is_colliding_with_new_shapes(
            &collision_world,
            old_aabb,
            new_aabb,
            vehicle.is_descending(),
        );

        if (MovementCollisionValidation {
            no_physics: false,
            moved_wrongly: fail,
            old_collision,
            new_collision,
        })
        .rejects()
        {
            if let Err(error) = vehicle.try_set_position(old_position) {
                log::warn!(
                    "Failed to roll vehicle {} back after rejected movement: {error}",
                    vehicle.id()
                );
            }
            vehicle.refresh_fluid_contact();
            vehicle.set_rotation((target_yaw, target_pitch));
            self.send_packet(Self::move_vehicle_packet_from_entity(vehicle.as_ref()));
            vehicle.remove_latest_movement_recording();
            return;
        }

        let client_delta = target_pos - old_position;
        match vehicle.apply_accepted_client_vehicle_movement(
            &world,
            AcceptedClientMovement {
                position: Some(target_pos),
                rotation: (target_yaw, target_pitch),
                on_ground: packet.on_ground,
                horizontal_collision: vehicle.horizontal_collision(),
                movement: client_delta,
                reset_fall_distance: false,
            },
        ) {
            Ok(AcceptedClientMovementOutcome::Applied) => {}
            Ok(AcceptedClientMovementOutcome::Handled) => return,
            Err(error) => {
                log::warn!(
                    "Rejected accepted vehicle movement for entity {}: {error}",
                    vehicle.id()
                );
                if let Err(rollback_error) = vehicle.try_set_position(old_position) {
                    log::warn!(
                        "Failed to roll vehicle {} back after rejected movement: {rollback_error}",
                        vehicle.id()
                    );
                }
                vehicle.refresh_fluid_contact();
                vehicle.set_rotation((target_yaw, target_pitch));
                self.send_packet(Self::move_vehicle_packet_from_entity(vehicle.as_ref()));
                vehicle.remove_latest_movement_recording();
                return;
            }
        }
        self.movement
            .lock()
            .set_last_known_client_movement(client_delta);
        world.chunk_map.update_player_status(self);
        self.record_client_vehicle_floating(
            &world,
            vehicle.as_ref(),
            move_delta.y,
            vehicle_rests_on_something,
        );
        self.movement
            .lock()
            .mark_vehicle_last_good_position(vehicle.id(), vehicle.position());
    }

    fn record_client_floating(
        &self,
        world: &World,
        y_dist: f64,
        player_stands_on_something: bool,
        is_spectator: bool,
        is_fall_flying: bool,
    ) {
        // TODO: Add auto-spin exemption when riptide/spin attack state exists.
        let can_violate_floating = PlayerFloatingValidation {
            y_dist,
            player_stands_on_something,
            is_spectator,
            server_allows_flight: self.config.allow_flight,
            may_fly: self.abilities.lock().may_fly,
            has_levitation: self.has_mob_effect(vanilla_mob_effects::LEVITATION),
            is_fall_flying,
        }
        .can_violate();

        let client_is_floating = can_violate_floating && Self::no_blocks_around_entity(world, self);
        self.movement
            .lock()
            .record_client_floating(client_is_floating);
    }

    fn record_client_vehicle_floating(
        &self,
        world: &World,
        vehicle: &dyn Entity,
        y_dist: f64,
        vehicle_rests_on_something: bool,
    ) {
        let client_is_floating = y_dist >= -0.03125
            && !vehicle_rests_on_something
            && !self.config.allow_flight
            && !vehicle.is_flying_vehicle()
            && !vehicle.is_no_gravity()
            && Self::no_blocks_around_entity(world, vehicle);
        self.movement
            .lock()
            .record_vehicle_client_floating(vehicle.id(), client_is_floating);
    }

    fn no_blocks_around_entity(world: &World, entity: &dyn Entity) -> bool {
        let block_query = entity
            .bounding_box()
            .inflate(0.0625)
            .expand_towards(DVec3::new(0.0, -0.55, 0.0));
        world.block_states_in_aabb_are_air(block_query)
    }

    /// Returns how long vanilla permits unsupported floating for this player's gravity.
    pub(super) fn maximum_flying_ticks(&self) -> i32 {
        Self::maximum_flying_ticks_for_gravity(self.get_gravity())
    }

    /// Returns how long vanilla permits unsupported floating for an entity gravity value.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "gravity threshold bounds the result far below i32::MAX"
    )]
    fn maximum_flying_ticks_for_gravity(gravity: f64) -> i32 {
        if gravity < 1.0E-5 {
            return i32::MAX;
        }

        let gravity_modifier = DEFAULT_GRAVITY / gravity;
        (80.0 * gravity_modifier.max(1.0)).ceil() as i32
    }

    /// Advances vanilla's floating violation tracker and disconnects when exceeded.
    pub(super) fn disconnect_if_floating_too_long(&self) -> bool {
        let should_count = !self.is_sleeping() && !self.is_passenger() && !self.is_dead_or_dying();
        let maximum_flying_ticks = self.maximum_flying_ticks();
        let should_disconnect = self
            .movement
            .lock()
            .tick_client_floating(should_count, maximum_flying_ticks);

        if should_disconnect {
            log::warn!(
                "{} was kicked for floating too long!",
                self.gameprofile.name
            );
            self.disconnect(translations::MULTIPLAYER_DISCONNECT_FLYING.msg());
        }

        should_disconnect
    }

    /// Advances vanilla's controlled-vehicle floating tracker and disconnects when exceeded.
    pub(super) fn disconnect_if_vehicle_floating_too_long(&self) -> bool {
        let Some(vehicle) = self.root_vehicle() else {
            self.movement.lock().clear_vehicle_for_tick();
            return false;
        };
        let controlled_by_player = vehicle
            .controlling_passenger()
            .is_some_and(|controller| controller.id() == self.id());
        if !controlled_by_player {
            self.movement.lock().clear_vehicle_for_tick();
            return false;
        }

        let maximum_flying_ticks = Self::maximum_flying_ticks_for_gravity(vehicle.get_gravity());
        let should_disconnect = self
            .movement
            .lock()
            .tick_vehicle_client_floating(vehicle.id(), maximum_flying_ticks);

        if should_disconnect {
            log::warn!(
                "{} was kicked for floating a vehicle too long!",
                self.gameprofile.name
            );
            self.disconnect(translations::MULTIPLAYER_DISCONNECT_FLYING.msg());
        }

        should_disconnect
    }

    /// Returns true if we're waiting for a teleport confirmation.
    #[must_use]
    pub fn is_awaiting_teleport(&self) -> bool {
        self.teleport_state.lock().is_awaiting()
    }

    /// Teleports the player to a new position.
    ///
    /// Sends a `CPlayerPosition` packet and waits for client acknowledgment.
    /// Until acknowledged, movement packets from the client will be rejected.
    ///
    /// Matches vanilla `ServerGamePacketListenerImpl.teleport()`.
    pub fn teleport(&self, pos: DVec3, yaw: f32, pitch: f32) -> Result<(), EntityMoveError> {
        self.try_set_position(pos)?;
        self.set_velocity(DVec3::ZERO);

        let new_id = {
            let mut tp = self.teleport_state.lock();
            tp.teleport_time = self.tick_count();
            let id = tp.next_id();
            tp.awaiting_position = Some(pos);
            id
        };

        self.set_rotation((yaw, pitch));
        self.set_old_position_to_current();
        {
            let mut movement = self.movement.lock();
            movement.reset_last_known_client_movement();
        }

        self.send_packet(CPlayerPosition::absolute(new_id, pos, yaw, pitch));
        Ok(())
    }

    /// Resets vanilla floating violation counters after a successful high-level teleport.
    pub(crate) fn reset_flying_ticks(&self) {
        self.movement.lock().reset_flying_ticks();
    }

    /// Handles a teleport acknowledgment from the client.
    ///
    /// Matches vanilla `ServerGamePacketListenerImpl.handleAcceptTeleportPacket()`.
    pub fn handle_accept_teleportation(&self, packet: SAcceptTeleportation) {
        let mut tp = self.teleport_state.lock();

        if let Some(pos) = tp.try_accept(packet.teleport_id) {
            drop(tp);
            if let Err(error) = self.try_set_position(pos) {
                log::warn!(
                    "Failed to commit accepted teleport for player entity {}: {error}",
                    self.id()
                );
                self.teleport_state.lock().awaiting_position = Some(pos);
                return;
            }
            self.set_old_position_to_current();
            let mut movement = self.movement.lock();
            movement.mark_last_good_position(pos);
            movement.reset_last_known_client_movement();
        } else if packet.teleport_id == tp.teleport_id && tp.awaiting_position.is_none() {
            drop(tp);
            self.disconnect(translations::MULTIPLAYER_DISCONNECT_INVALID_PLAYER_MOVEMENT.msg());
        }
    }

    /// Returns the latest vanilla client input snapshot.
    #[must_use]
    pub fn last_client_input(&self) -> PlayerInput {
        self.movement.lock().last_client_input()
    }

    /// Returns vanilla `ServerPlayer.getLastClientMoveIntent()`.
    #[must_use]
    pub fn last_client_move_intent(&self) -> DVec3 {
        get_input_vector(
            self.last_client_input().movement_input(),
            1.0,
            self.rotation().0,
        )
    }

    /// Handles a player input packet (movement keys, sneaking, sprinting).
    pub fn handle_player_input(&self, packet: SPlayerInput) {
        // Vanilla stores the input unconditionally before the guard check.
        let input = PlayerInput::from_flags(packet.flags);
        self.movement.lock().set_last_client_input(input);

        if !self.has_client_loaded() {
            return;
        }

        // TODO: Vanilla calls this.player.resetLastActionTime() here which sets
        // lastActionTime = Util.getMillis(), preventing idle-kick. Add when idle-kick system is implemented.

        self.set_crouching(input.shift());
    }

    /// Handles a player command packet (sprinting, elytra, leaving bed, etc).
    pub fn handle_player_command(&self, packet: SPlayerCommand) {
        if !self.has_client_loaded() {
            return;
        }

        if packet.entity_id != self.id() {
            log::warn!(
                "Player {} (eid {}) sent SPlayerCommand with mismatched entity_id {}",
                self.gameprofile.name,
                self.id(),
                packet.entity_id
            );
            return;
        }

        // TODO: Vanilla calls this.player.resetLastActionTime() here which sets
        // noActionTime = 0, preventing idle-kick. Add when idle-kick system is implemented.

        match packet.action {
            PlayerCommandAction::StartSprinting => {
                self.set_sprinting(true);
            }
            PlayerCommandAction::StopSprinting => {
                self.set_sprinting(false);
            }
            PlayerCommandAction::StartFallFlying => {
                if !self.try_to_start_fall_flying() {
                    self.stop_fall_flying();
                }
            }
            PlayerCommandAction::LeaveBed => {
                if self.is_sleeping() {
                    self.stop_sleeping();
                    // TODO: Full bed wake-up logic:
                    //   - set bed block OCCUPIED property to false
                    //   - compute stand-up position via BedBlock::findStandUpPosition
                    //   - teleport player + set rotation toward bed
                    //   - set pose to Standing, clear sleeping pos entity data
                    //   - update server sleeping player list (for sleep-skip)
                    //   - set sleepCounter = 100
                    //   - set awaiting_position_from_client
                    // Blocked on: bed block properties, sleeping pos entity data
                }
            }
            PlayerCommandAction::StartRidingJump
            | PlayerCommandAction::StopRidingJump
            | PlayerCommandAction::OpenVehicleInventory => {
                // TODO: Implement once controlled vehicle jumping and vehicle inventory interfaces exist.
            }
        }

        // Dirty shared flags are synced once per tick by sync_entity_data().
    }
}

#[cfg(test)]
#[expect(clippy::float_cmp, reason = "exact match against vanilla test vectors")]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_horizontal() {
        assert_eq!(clamp_horizontal(0.0), 0.0);
        assert_eq!(clamp_horizontal(1e8), CLAMP_HORIZONTAL);
        assert_eq!(clamp_horizontal(-1e8), -CLAMP_HORIZONTAL);
    }

    #[test]
    fn test_clamp_vertical() {
        assert_eq!(clamp_vertical(0.0), 0.0);
        assert_eq!(clamp_vertical(1e8), CLAMP_VERTICAL);
        assert_eq!(clamp_vertical(-1e8), -CLAMP_VERTICAL);
    }

    #[test]
    fn test_wrap_degrees() {
        assert_eq!(wrap_degrees(181.0), -179.0);
        assert_eq!(wrap_degrees(-181.0), 179.0);
        assert_eq!(wrap_degrees(90.0), 90.0);
    }

    #[test]
    fn floating_validation_exempts_levitation_like_vanilla() {
        let validation = PlayerFloatingValidation {
            y_dist: 0.0,
            player_stands_on_something: false,
            is_spectator: false,
            server_allows_flight: false,
            may_fly: false,
            has_levitation: false,
            is_fall_flying: false,
        };

        assert!(validation.can_violate());
        assert!(
            !PlayerFloatingValidation {
                y_dist: -0.0784,
                ..validation
            }
            .can_violate()
        );
        assert!(
            !PlayerFloatingValidation {
                has_levitation: true,
                ..validation
            }
            .can_violate()
        );
    }
}
