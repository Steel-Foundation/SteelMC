//! Player movement physics and validation.
//!
//! This module handles server-side movement simulation and anti-cheat checks.
//! It implements collision detection and physics similar to vanilla Minecraft.

use std::sync::{Arc, atomic::Ordering};

use glam::DVec3;
use steel_protocol::packets::game::{
    CEntityPositionSync, CMoveEntityPosRot, CMoveEntityRot, CPlayerPosition, CRotateHead,
    PlayerCommandAction, SAcceptTeleportation, SMovePlayer, SPlayerCommand, SPlayerInput,
    to_angle_byte,
};
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_attributes;
use steel_registry::vanilla_game_rules::{ELYTRA_MOVEMENT_CHECK, PLAYER_MOVEMENT_CHECK};
use steel_utils::types::GameType;
use steel_utils::{ChunkPos, WorldAabb, translations};

use crate::entity::{Entity, LivingEntity};
use crate::physics::{CollisionWorld, MoverType, WorldCollisionProvider, join_is_not_empty};
use crate::player::Player;
use crate::player::food_data::food_constants;
use crate::world::World;

/// Small epsilon for AABB deflation (matches vanilla 1.0E-5).
pub const COLLISION_EPSILON: f64 = 1.0E-5;

/// Default gravity for players (blocks/tick²). Vanilla uses 0.08.
pub const DEFAULT_GRAVITY: f64 = 0.08;

/// Maximum movement speed threshold for normal movement (meters per tick squared).
pub const SPEED_THRESHOLD_NORMAL: f64 = 100.0;
/// Maximum movement speed threshold for elytra flight (meters per tick squared).
pub const SPEED_THRESHOLD_FLYING: f64 = 300.0;

/// Movement error threshold - if player ends up more than this far from target, reject.
/// Matches vanilla's 0.0625 (1/16 of a block squared).
pub const MOVEMENT_ERROR_THRESHOLD: f64 = 0.0625;

/// Horizontal position clamping limit (matches vanilla).
pub const CLAMP_HORIZONTAL: f64 = 3.0E7;
/// Vertical position clamping limit (matches vanilla).
pub const CLAMP_VERTICAL: f64 = 2.0E7;

/// Y-axis tolerance for movement error checks.
/// Vanilla ignores Y differences within this range after physics simulation.
pub const Y_TOLERANCE: f64 = 0.5;

/// Post-impulse grace period in ticks (vanilla uses ~10-20 ticks).
pub const IMPULSE_GRACE_TICKS: i32 = 20;

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

#[must_use]
const fn bottom_center(aabb: WorldAabb) -> DVec3 {
    DVec3::new(
        f64::midpoint(aabb.min_x(), aabb.max_x()),
        aabb.min_y(),
        f64::midpoint(aabb.min_z(), aabb.max_z()),
    )
}

/// Checks if an entity box is colliding with blocks.
#[must_use]
pub fn has_block_collision(world: &Arc<World>, aabb: WorldAabb) -> bool {
    let collision_world = WorldCollisionProvider::new(world);
    !collision_world
        .get_block_collisions(&aabb.deflate(COLLISION_EPSILON))
        .is_empty()
}

/// Checks if moving from `old_aabb` to `new_aabb` would cause collision with new blocks.
///
/// This allows movement when already stuck in blocks (e.g., sand fell on player).
/// Only returns true if the new position collides with blocks that the old position
/// did not collide with.
///
/// Uses the physics engine's `join_is_not_empty` for proper collision detection.
///
/// Matches vanilla `ServerGamePacketListenerImpl.isEntityCollidingWithAnythingNew()`.
#[must_use]
pub fn is_colliding_with_new_blocks(
    world: &Arc<World>,
    old_aabb: WorldAabb,
    new_aabb: WorldAabb,
) -> bool {
    let collision_world = WorldCollisionProvider::new(world);
    let old_shape = old_aabb.deflate(COLLISION_EPSILON);
    let collisions = collision_world.get_pre_move_collisions(
        &new_aabb.deflate(COLLISION_EPSILON),
        bottom_center(old_aabb),
    );

    for collision_aabb in &collisions {
        if !join_is_not_empty(collision_aabb, &old_shape) {
            return true;
        }
    }

    false
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
            tp.teleport_time = self.tick_count.load(Ordering::Relaxed);
            return false;
        };

        let current_tick = self.tick_count.load(Ordering::Relaxed);

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

    /// Marks that an impulse (knockback, etc.) was applied.
    pub fn apply_impulse(&self) {
        self.movement.lock().last_impulse_tick = self.tick_count.load(Ordering::Relaxed);
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

        if !self.client_loaded.load(Ordering::Relaxed) {
            return;
        }

        let prev_pos = self.movement.lock().position_sync.last_sent_position();
        let start_pos = self.position();
        let game_mode = self.game_mode.load();
        let state = self.entity_state_snapshot();
        let (is_sleeping, is_fall_flying) = (state.sleeping, state.fall_flying);
        let was_on_ground = self.on_ground();
        let is_spectator = game_mode == GameType::Spectator;
        let is_creative = game_mode == GameType::Creative;
        let world = self.get_world();
        let tick_runs_normally = world.tick_runs_normally();
        let mut accepted_pos = prev_pos;
        let mut client_delta = DVec3::ZERO;

        if packet.has_pos {
            let target_pos = DVec3::new(
                clamp_horizontal(packet.position.x),
                clamp_vertical(packet.position.y),
                clamp_horizontal(packet.position.z),
            );
            let (first_good, last_good) = {
                let mv = self.movement.lock();
                (mv.first_good_position, mv.last_good_position)
            };

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
                        mv.received_move_packet_count += 1;
                        mv.received_move_packet_count - mv.known_move_packet_count
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
                let moved_upwards = move_delta.y > 0.0;

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

                let simulated_pos = self.position();
                let error_x = target_pos.x - simulated_pos.x;
                let mut error_y = target_pos.y - simulated_pos.y;
                let error_z = target_pos.z - simulated_pos.z;
                if error_y > -Y_TOLERANCE || error_y < Y_TOLERANCE {
                    error_y = 0.0;
                }

                let error_dist_sq = error_x * error_x + error_y * error_y + error_z * error_z;
                let in_impulse_grace = {
                    let mv = self.movement.lock();
                    let current_tick = self.tick_count.load(Ordering::Relaxed);
                    current_tick.wrapping_sub(mv.last_impulse_tick) < IMPULSE_GRACE_TICKS
                };
                let fail = error_dist_sq > MOVEMENT_ERROR_THRESHOLD
                    && !is_creative
                    && !is_spectator
                    && !in_impulse_grace;

                let new_aabb = self.bounding_box().move_vec(target_pos - self.position());
                let old_collision = has_block_collision(&world, old_aabb);
                let new_collision = is_colliding_with_new_blocks(&world, old_aabb, new_aabb);

                if (fail && !old_collision) || new_collision {
                    self.teleport(
                        start_pos.x,
                        start_pos.y,
                        start_pos.z,
                        target_yaw,
                        target_pitch,
                    );
                    return;
                }

                self.movement.lock().last_good_position = target_pos;

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
        }
        self.set_rotation((target_yaw, target_pitch));
        self.base().set_on_ground_with_movement(
            packet.on_ground,
            packet.horizontal_collision,
            client_delta,
        );

        let pos = if packet.has_pos {
            accepted_pos
        } else {
            prev_pos
        };
        let (yaw, pitch) = (target_yaw, target_pitch);

        if packet.has_pos || packet.has_rot {
            let new_chunk = ChunkPos::from_entity_pos(pos);

            if packet.has_pos {
                let (sync_delay, last_on_ground, delta) = {
                    let mut mv = self.movement.lock();
                    let d = mv.position_sync.advance_sync_delay();
                    (
                        d,
                        mv.position_sync.last_sent_on_ground(),
                        mv.position_sync.packed_delta(pos),
                    )
                };
                let on_ground_changed = last_on_ground != packet.on_ground;
                let force_sync = sync_delay > 400 || on_ground_changed;

                if let Some((dx, dy, dz)) = delta {
                    if force_sync {
                        {
                            let mut mv = self.movement.lock();
                            mv.position_sync.mark_full_sent(pos, packet.on_ground);
                        }

                        let delta = self.velocity();
                        let sync_packet = CEntityPositionSync {
                            entity_id: self.id(),
                            x: pos.x,
                            y: pos.y,
                            z: pos.z,
                            velocity_x: delta.x,
                            velocity_y: delta.y,
                            velocity_z: delta.z,
                            yaw,
                            pitch,
                            on_ground: packet.on_ground,
                        };
                        world.broadcast_to_nearby(new_chunk, sync_packet, Some(self.id()));
                    } else {
                        self.movement
                            .lock()
                            .position_sync
                            .mark_delta_sent(pos, packet.on_ground);

                        let move_packet = CMoveEntityPosRot {
                            entity_id: self.id(),
                            dx,
                            dy,
                            dz,
                            y_rot: to_angle_byte(yaw),
                            x_rot: to_angle_byte(pitch),
                            on_ground: packet.on_ground,
                        };
                        world.broadcast_to_nearby(new_chunk, move_packet, Some(self.id()));
                    }
                } else {
                    {
                        let mut mv = self.movement.lock();
                        mv.position_sync.mark_full_sent(pos, packet.on_ground);
                    }

                    let delta = self.velocity();
                    let sync_packet = CEntityPositionSync {
                        entity_id: self.id(),
                        x: pos.x,
                        y: pos.y,
                        z: pos.z,
                        velocity_x: delta.x,
                        velocity_y: delta.y,
                        velocity_z: delta.z,
                        yaw,
                        pitch,
                        on_ground: packet.on_ground,
                    };
                    world.broadcast_to_nearby(new_chunk, sync_packet, Some(self.id()));
                }
            } else {
                let rot_packet = CMoveEntityRot {
                    entity_id: self.id(),
                    y_rot: to_angle_byte(yaw),
                    x_rot: to_angle_byte(pitch),
                    on_ground: packet.on_ground,
                };
                world.broadcast_to_nearby(new_chunk, rot_packet, Some(self.id()));
            }

            if packet.has_rot {
                let head_packet = CRotateHead {
                    entity_id: self.id(),
                    head_y_rot: to_angle_byte(yaw),
                };
                world.broadcast_to_nearby(new_chunk, head_packet, Some(self.id()));
            }

            let mut mv = self.movement.lock();
            mv.prev_rotation = (yaw, pitch);
        }
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

    /// Applies gravity to the player's velocity.
    ///
    /// Matches vanilla `Entity.applyGravity()` and `LivingEntity.travel()`.
    /// Gravity is not applied when:
    /// - Player is on the ground
    /// - Player is in spectator mode (no physics)
    /// - Player is in creative mode and flying
    /// - Player is fall flying (elytra - uses different physics)
    pub(super) fn apply_gravity(&self) {
        let is_fall_flying = self.is_fall_flying();
        let on_ground = self.on_ground();
        let game_mode = self.game_mode.load();
        let is_spectator = game_mode == GameType::Spectator;
        let is_creative_flying = game_mode == GameType::Creative; // TODO: check actual flying state

        if on_ground || is_spectator || is_creative_flying || is_fall_flying {
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
            tp.teleport_time = self.tick_count.load(Ordering::Relaxed);
            let id = tp.next_id();
            tp.awaiting_position = Some(pos);
            id
        };

        self.set_position(pos);
        self.set_rotation((yaw, pitch));

        self.send_packet(CPlayerPosition::absolute(new_id, x, y, z, yaw, pitch));
    }

    /// Handles a teleport acknowledgment from the client.
    ///
    /// Matches vanilla `ServerGamePacketListenerImpl.handleAcceptTeleportPacket()`.
    pub fn handle_accept_teleportation(&self, packet: SAcceptTeleportation) {
        let mut tp = self.teleport_state.lock();

        if let Some(pos) = tp.try_accept(packet.teleport_id) {
            self.set_position(pos);
            self.movement.lock().last_good_position = pos;
        } else if packet.teleport_id == tp.teleport_id && tp.awaiting_position.is_none() {
            drop(tp);
            self.disconnect(translations::MULTIPLAYER_DISCONNECT_INVALID_PLAYER_MOVEMENT.msg());
        }
    }

    /// Handles a player input packet (movement keys, sneaking, sprinting).
    pub fn handle_player_input(&self, packet: SPlayerInput) {
        // Vanilla stores the input unconditionally before the guard check.
        // SteelMC doesn't have setLastClientInput yet, so we skip that.

        if !self.client_loaded.load(Ordering::Relaxed) {
            return;
        }

        // TODO: Vanilla calls this.player.resetLastActionTime() here which sets
        // lastActionTime = Util.getMillis(), preventing idle-kick. Add when idle-kick system is implemented.

        self.set_crouching(packet.shift());
    }

    /// Handles a player command packet (sprinting, elytra, leaving bed, etc).
    // this is just temporary there because the logic is not yet implemented complete for the other branches
    #[expect(
        clippy::match_same_arms,
        reason = "There is still a TODO there, this will eventually go away by itself."
    )]
    pub fn handle_player_command(&self, packet: SPlayerCommand) {
        if !self.client_loaded.load(Ordering::Relaxed) {
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
                //   - not in water, not a passenger
                //   - no Levitation effect
                //   - at least one equipped item has GLIDER component in correct slot
                //     and won't break on next damage
                //   - not in creative flight
                // If validation fails, call stop_fall_flying() (toggle shared flag 7)
                // Also needs tick-based updateFallFlying():
                //   - re-validate canGlide() every tick
                //   - damage a random glider item every 20 ticks
                //   - emit ELYTRA_GLIDE game event every 10 ticks
                // Blocked on: equipment checks working end-to-end, potion effects,
                //             fluid detection, passenger/vehicle system
                self.set_fall_flying(true);
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
            PlayerCommandAction::StartRidingJump => {
                // TODO: horse jump — check getControlledVehicle() is PlayerRideableJumping,
                //       validate canJump() && data > 0, call handleStartJump(data)
                // Blocked on: vehicle/entity system
            }
            PlayerCommandAction::StopRidingJump => {
                // TODO: stop horse jump — call handleStopJump() on controlled vehicle
                // Blocked on: vehicle/entity system
            }
            PlayerCommandAction::OpenVehicleInventory => {
                // TODO: open vehicle inventory — check getVehicle() is HasCustomInventoryScreen
                // Blocked on: vehicle/entity system
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
