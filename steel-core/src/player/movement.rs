//! Player movement physics and validation.
//!
//! This module handles server-side movement simulation and anti-cheat checks.
//! It implements collision detection and physics similar to vanilla Minecraft.

use std::sync::Arc;

use glam::DVec3;
use steel_protocol::packets::game::{
    CPlayerPosition, PlayerCommandAction, SAcceptTeleportation, SMovePlayer, SPlayerCommand,
    SPlayerInput,
};
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_attributes;
use steel_registry::vanilla_game_rules::{ELYTRA_MOVEMENT_CHECK, PLAYER_MOVEMENT_CHECK};
use steel_utils::types::GameType;
use steel_utils::{ChunkPos, translations};

use crate::entity::{Entity, EntityMovementSyncUpdate, LivingEntity};
use crate::physics::{
    MOVEMENT_ERROR_THRESHOLD, MovementCollisionValidation, MoverType, WorldCollisionProvider,
    has_collision, is_colliding_with_new_shapes, movement_error_delta, vanilla_post_move_y_dist,
};
use crate::player::Player;
use crate::player::food_data::food_constants;
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
struct AcceptedMovementBroadcast {
    has_pos: bool,
    has_rot: bool,
    pos: DVec3,
    yaw: f32,
    pitch: f32,
    on_ground: bool,
    client_delta: DVec3,
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
            tp.teleport_time = current_tick;
            let teleport_id = tp.teleport_id;
            drop(tp);

            let (yaw, pitch) = self.rotation();
            self.send_packet(CPlayerPosition::absolute(
                teleport_id,
                pos.x,
                pos.y,
                pos.z,
                yaw,
                pitch,
            ));
        }
        true
    }

    /// Applies vanilla post-impulse movement validation grace.
    pub fn apply_post_impulse_grace_time(&self, ticks: i32) {
        self.movement.lock().apply_post_impulse_grace_time(ticks);
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

        let prev_pos = self.movement.lock().last_sent_position();
        let start_pos = self.position();
        let game_mode = self.game_mode();
        let state = self.entity_state_snapshot();
        let (is_sleeping, is_fall_flying) = (state.sleeping, state.fall_flying);
        let was_on_ground = self.on_ground();
        let is_spectator = game_mode == GameType::Spectator;
        let is_creative = game_mode == GameType::Creative;
        let world = self.get_world();
        let tick_runs_normally = world.tick_runs_normally();
        let mut accepted_pos = prev_pos;
        let mut client_delta = DVec3::ZERO;
        let mut moved_upwards = false;
        let mut floating_check = None;

        if self.is_passenger() {
            self.set_rotation((target_yaw, target_pitch));
            self.broadcast_accepted_movement(
                &world,
                AcceptedMovementBroadcast {
                    has_pos: false,
                    has_rot: packet.has_rot,
                    pos: prev_pos,
                    yaw: target_yaw,
                    pitch: target_pitch,
                    on_ground: self.on_ground(),
                    client_delta,
                },
            );
            return;
        }

        if packet.has_pos {
            let target_pos = DVec3::new(
                clamp_horizontal(packet.position.x),
                clamp_vertical(packet.position.y),
                clamp_horizontal(packet.position.z),
            );
            let (first_good, last_good) = self.movement.lock().good_positions();

            if is_sleeping {
                let dx = target_pos.x - first_good.x;
                let dy = target_pos.y - first_good.y;
                let dz = target_pos.z - first_good.z;
                let moved_dist_sq = dx * dx + dy * dy + dz * dz;

                if moved_dist_sq > 1.0 {
                    self.teleport(
                        start_pos.x,
                        start_pos.y,
                        start_pos.z,
                        target_yaw,
                        target_pitch,
                    );
                    return;
                }
            } else {
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
                            self.teleport(
                                start_pos.x,
                                start_pos.y,
                                start_pos.z,
                                current_rotation.0,
                                current_rotation.1,
                            );
                            return;
                        }
                    }
                }

                let old_aabb = self.bounding_box();
                let move_delta = target_pos - last_good;
                moved_upwards = move_delta.y > 0.0;
                let player_stands_on_something = self.vertical_collision_below();

                if was_on_ground && !packet.on_ground && moved_upwards {
                    if self.is_sprinting() {
                        self.cause_food_exhaustion(food_constants::EXHAUSTION_SPRINT_JUMP);
                    } else {
                        self.cause_food_exhaustion(food_constants::EXHAUSTION_JUMP);
                    }
                }

                let Some(_move_result) = self.move_entity(MoverType::Player, move_delta) else {
                    self.teleport(
                        start_pos.x,
                        start_pos.y,
                        start_pos.z,
                        target_yaw,
                        target_pitch,
                    );
                    return;
                };

                let error_delta = movement_error_delta(target_pos, self.position());
                let error_dist_sq = error_delta.length_squared();
                let in_impulse_grace = {
                    let mv = self.movement.lock();
                    mv.is_in_post_impulse_grace_time()
                };
                let fail = error_dist_sq > MOVEMENT_ERROR_THRESHOLD
                    && !is_creative
                    && !is_spectator
                    && !in_impulse_grace;

                let new_aabb = self.bounding_box().move_vec(target_pos - self.position());
                let collision_world = WorldCollisionProvider::for_entity(&world, self);
                let old_collision = has_collision(&collision_world, old_aabb);
                let new_collision = is_colliding_with_new_shapes(
                    &collision_world,
                    old_aabb,
                    new_aabb,
                    self.is_crouching(),
                );

                if (MovementCollisionValidation {
                    no_physics: self.no_physics(),
                    moved_wrongly: fail,
                    old_collision,
                    new_collision,
                })
                .rejects()
                {
                    self.teleport(
                        start_pos.x,
                        start_pos.y,
                        start_pos.z,
                        target_yaw,
                        target_pitch,
                    );
                    self.do_check_fall_damage(DVec3::ZERO, packet.on_ground, &world);
                    self.remove_latest_movement_recording();
                    return;
                }

                floating_check = Some((
                    player_stands_on_something,
                    vanilla_post_move_y_dist(target_pos.y, self.position().y),
                ));
                self.movement.lock().mark_last_good_position(target_pos);

                if packet.on_ground && self.is_sprinting() {
                    let dx = move_delta.x;
                    let dz = move_delta.z;

                    let cm = ((dx * dx + dz * dz).sqrt() as f32 * 100.0).round() as i32;
                    if cm > 0 {
                        self.cause_food_exhaustion(
                            food_constants::EXHAUSTION_SPRINT * cm as f32 * 0.01,
                        );
                    }
                }
            }

            accepted_pos = target_pos;
            client_delta = accepted_pos - start_pos;
        }

        if packet.has_pos {
            self.set_position(accepted_pos);
            self.refresh_fluid_contact();
        }
        if let Some((player_stands_on_something, y_dist)) = floating_check {
            self.record_client_floating(
                &world,
                y_dist,
                player_stands_on_something,
                is_spectator,
                is_fall_flying,
            );
        }
        self.set_rotation((target_yaw, target_pitch));
        self.set_on_ground_with_movement(
            packet.on_ground,
            packet.horizontal_collision,
            client_delta,
        );
        if self.do_check_fall_damage(client_delta, packet.on_ground, &world) {
            return;
        }
        if moved_upwards {
            self.reset_fall_distance();
        }

        self.broadcast_accepted_movement(
            &world,
            AcceptedMovementBroadcast {
                has_pos: packet.has_pos,
                has_rot: packet.has_rot,
                pos: if packet.has_pos {
                    accepted_pos
                } else {
                    prev_pos
                },
                yaw: target_yaw,
                pitch: target_pitch,
                on_ground: packet.on_ground,
                client_delta,
            },
        );
    }

    fn broadcast_accepted_movement(&self, world: &Arc<World>, movement: AcceptedMovementBroadcast) {
        if !movement.has_pos && !movement.has_rot {
            return;
        }

        let new_chunk = ChunkPos::from_entity_pos(movement.pos);
        let body_rotation = (movement.yaw, movement.pitch);
        let packets = {
            let mut state = self.movement.lock();
            state.set_last_known_client_movement(movement.client_delta);
            state.record_accepted_movement_sync(EntityMovementSyncUpdate {
                entity_id: self.id(),
                has_position: movement.has_pos,
                has_rotation: movement.has_rot,
                position: movement.pos,
                velocity: self.velocity(),
                body_rotation,
                head_yaw: movement.yaw,
                on_ground: movement.on_ground,
            })
        };

        packets.for_each(|packet| {
            world.broadcast_movement_sync_to_nearby(new_chunk, packet, Some(self.id()));
        });
    }

    fn record_client_floating(
        &self,
        world: &World,
        y_dist: f64,
        player_stands_on_something: bool,
        is_spectator: bool,
        is_fall_flying: bool,
    ) {
        let may_fly = self.abilities.lock().may_fly;
        // TODO: Add levitation and auto-spin exemptions when those systems exist.
        let can_violate_floating = y_dist >= -0.03125
            && !player_stands_on_something
            && !is_spectator
            && !self.config.allow_flight
            && !may_fly
            && !is_fall_flying;

        let client_is_floating = can_violate_floating && self.no_blocks_around(world);
        self.movement
            .lock()
            .record_client_floating(client_is_floating);
    }

    fn no_blocks_around(&self, world: &World) -> bool {
        let block_query = self
            .bounding_box()
            .inflate(0.0625)
            .expand_towards(DVec3::new(0.0, -0.55, 0.0));
        world.block_states_in_aabb_are_air(block_query)
    }

    /// Returns the player's current gravity value.
    ///
    /// Matches vanilla `LivingEntity.getGravity()` which reads from `Attributes.GRAVITY`.
    /// Default is 0.08 blocks/tick².
    fn get_gravity(&self) -> f64 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::GRAVITY)
            .unwrap_or(DEFAULT_GRAVITY)
    }

    /// Returns how long vanilla permits unsupported floating for this player's gravity.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "gravity threshold bounds the result far below i32::MAX"
    )]
    pub(super) fn maximum_flying_ticks(&self) -> i32 {
        let gravity = self.get_gravity();
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

    /// Applies gravity to the player's velocity.
    ///
    /// Matches vanilla `Entity.applyGravity()` and `LivingEntity.travel()`.
    /// Gravity is not applied when:
    /// - Player is on the ground
    /// - Player is in spectator mode (no physics)
    /// - Player abilities are currently flying
    /// - Player is fall flying (elytra - uses different physics)
    pub(super) fn apply_gravity(&self) {
        let is_fall_flying = self.is_fall_flying();
        let on_ground = self.on_ground();
        let game_mode = self.game_mode();
        let is_spectator = game_mode == GameType::Spectator;
        let is_flying = self.is_flying();
        let is_passenger = self.is_passenger();

        if is_flying && !is_passenger {
            self.reset_fall_distance();
        }

        if on_ground || is_spectator || is_flying || is_fall_flying || is_passenger {
            return;
        }

        let gravity = self.get_gravity();
        if gravity != 0.0 {
            let mut velocity = self.velocity();
            velocity.y -= gravity;
            self.set_velocity(velocity);
        }
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
    pub fn teleport(&self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        let pos = DVec3::new(x, y, z);

        let new_id = {
            let mut tp = self.teleport_state.lock();
            tp.teleport_time = self.tick_count();
            let id = tp.next_id();
            tp.awaiting_position = Some(pos);
            id
        };

        self.set_position(pos);
        self.set_rotation((yaw, pitch));
        self.set_old_position_to_current();
        {
            let mut movement = self.movement.lock();
            movement.reset_last_known_client_movement();
            movement.reset_flying_ticks();
        }

        self.send_packet(CPlayerPosition::absolute(new_id, x, y, z, yaw, pitch));
    }

    /// Handles a teleport acknowledgment from the client.
    ///
    /// Matches vanilla `ServerGamePacketListenerImpl.handleAcceptTeleportPacket()`.
    pub fn handle_accept_teleportation(&self, packet: SAcceptTeleportation) {
        let mut tp = self.teleport_state.lock();

        if let Some(pos) = tp.try_accept(packet.teleport_id) {
            self.set_position(pos);
            self.set_old_position_to_current();
            let mut movement = self.movement.lock();
            movement.mark_last_good_position(pos);
            movement.reset_last_known_client_movement();
            movement.reset_flying_ticks();
        } else if packet.teleport_id == tp.teleport_id && tp.awaiting_position.is_none() {
            drop(tp);
            self.disconnect(translations::MULTIPLAYER_DISCONNECT_INVALID_PLAYER_MOVEMENT.msg());
        }
    }

    /// Handles a player input packet (movement keys, sneaking, sprinting).
    pub fn handle_player_input(&self, packet: SPlayerInput) {
        // Vanilla stores the input unconditionally before the guard check.
        // SteelMC doesn't have setLastClientInput yet, so we skip that.

        if !self.has_client_loaded() {
            return;
        }

        // TODO: Vanilla calls this.player.resetLastActionTime() here which sets
        // lastActionTime = Util.getMillis(), preventing idle-kick. Add when idle-kick system is implemented.

        self.set_crouching(packet.shift());
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
                // TODO: Full canGlide() checks once the required systems exist:
                //   - no Levitation effect
                //   - at least one equipped item has GLIDER component in correct slot
                //     and won't break on next damage
                // If validation fails, call stop_fall_flying() (toggle shared flag 7)
                // Also needs tick-based updateFallFlying():
                //   - re-validate canGlide() every tick
                //   - damage a random glider item every 20 ticks
                //   - emit ELYTRA_GLIDE game event every 10 ticks
                // Blocked on: equipment checks working end-to-end and potion effects.
                if !self.is_fall_flying()
                    && !self.on_ground()
                    && !self.is_passenger()
                    && !self.is_flying()
                    && !self.is_in_water()
                {
                    self.set_fall_flying(true);
                } else {
                    self.set_fall_flying(false);
                }
            }
            PlayerCommandAction::LeaveBed => {
                if self.is_sleeping() {
                    self.set_sleeping(false);
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

        // Shared flags are updated once per tick in tick() → update_shared_flags().
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
}
