//! Player movement physics and validation.
//!
//! This module handles server-side movement simulation and anti-cheat checks.
//! It implements collision detection and physics similar to vanilla Minecraft.

use std::sync::{Arc, atomic::Ordering};

use glam::DVec3;
use steel_protocol::packets::game::{
    CEntityPositionSync, CMoveEntityPosRot, CMoveEntityRot, CPlayerPosition, CRotateHead,
    PlayerCommandAction, SAcceptTeleportation, SMovePlayer, SPlayerCommand, SPlayerInput,
    calc_delta, to_angle_byte,
};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::shapes::AABBd;
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_game_rules::{ELYTRA_MOVEMENT_CHECK, PLAYER_MOVEMENT_CHECK};
use steel_registry::{vanilla_attributes, vanilla_entities};
use steel_utils::types::GameType;
use steel_utils::{BlockPos, ChunkPos, translations};

use crate::entity::LivingEntity;
use crate::physics::{
    CollisionWorld, EntityPhysicsState, MoverType, WorldCollisionProvider, join_is_not_empty,
    move_entity,
};
use crate::player::Player;
use crate::player::food_data::food_constants;
use crate::world::World;

/// Player bounding box width (from entity type registry).
pub const PLAYER_WIDTH: f64 = vanilla_entities::PLAYER.dimensions.width as f64;
/// Player bounding box height (from entity type registry).
pub const PLAYER_HEIGHT: f64 = vanilla_entities::PLAYER.dimensions.height as f64;

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

/// Creates a player bounding box at the given position.
#[must_use]
pub fn make_player_aabb(pos: DVec3) -> AABBd {
    AABBd::entity_box(pos.x, pos.y, pos.z, PLAYER_WIDTH / 2.0, PLAYER_HEIGHT)
}

/// Creates a player bounding box at the given position, deflated by the collision epsilon.
#[must_use]
pub fn make_player_aabb_deflated(pos: DVec3) -> AABBd {
    make_player_aabb(pos).deflate(COLLISION_EPSILON)
}

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

// ============================================================================
// Movement Simulation (using physics engine)
// ============================================================================

/// Result of a movement simulation.
#[derive(Debug, Clone)]
pub struct MoveResult {
    /// The actual movement after collision resolution.
    pub movement: DVec3,
    /// The final position after movement.
    pub position: DVec3,
    /// Whether there was a collision on the X axis.
    pub collision_x: bool,
    /// Whether there was a collision on the Y axis.
    pub collision_y: bool,
    /// Whether there was a collision on the Z axis.
    pub collision_z: bool,
    /// Whether the player is on the ground after this movement.
    pub on_ground: bool,
}

/// Simulates player movement with collision detection.
///
/// This is the server-side equivalent of vanilla's `Entity.move()`.
/// It takes a starting position and desired movement delta, then returns
/// where the player would actually end up after collision resolution.
///
/// Uses the new physics engine with step-up and sneak-edge prevention.
///
/// # Arguments
/// * `world` - The world to check collisions against
/// * `start_pos` - The player's starting position
/// * `delta` - The desired movement vector
/// * `is_crouching` - Whether the player is sneaking (for edge prevention)
/// * `on_ground` - Whether the player is currently on ground (affects step-up)
///
/// # Returns
/// A `MoveResult` containing the resolved movement and collision info.
#[must_use]
pub fn simulate_move(
    world: &Arc<World>,
    start_pos: DVec3,
    delta: DVec3,
    is_crouching: bool,
    on_ground: bool,
) -> MoveResult {
    // Create physics state for the player
    let mut state = EntityPhysicsState::new(start_pos, &vanilla_entities::PLAYER);
    state.is_crouching = is_crouching;
    state.on_ground = on_ground;

    // Create collision provider
    let collision_world = WorldCollisionProvider::new(world);

    // Run physics simulation
    let physics_result = move_entity(&state, delta, MoverType::SelfMovement, &collision_world);

    // Convert physics result to movement result
    MoveResult {
        movement: physics_result.actual_movement,
        position: physics_result.final_position,
        collision_x: physics_result.horizontal_collision,
        collision_y: physics_result.vertical_collision,
        collision_z: physics_result.horizontal_collision, // Horizontal includes both X and Z
        on_ground: physics_result.on_ground,
    }
}

/// Checks if a player at the given position is colliding with any blocks.
///
/// Used to allow movement when already stuck in blocks.
#[must_use]
pub fn is_in_collision(world: &Arc<World>, pos: DVec3) -> bool {
    let aabb = make_player_aabb_deflated(pos);

    let min_x = aabb.min_x.floor() as i32;
    let max_x = aabb.max_x.ceil() as i32;
    let min_y = aabb.min_y.floor() as i32;
    let max_y = aabb.max_y.ceil() as i32;
    let min_z = aabb.min_z.floor() as i32;
    let max_z = aabb.max_z.ceil() as i32;

    for bx in min_x..max_x {
        for by in min_y..max_y {
            for bz in min_z..max_z {
                let block_pos = BlockPos::new(bx, by, bz);
                let block_state = world.get_block_state(block_pos);
                let collision_shape = block_state.get_collision_shape();

                for block_aabb in collision_shape {
                    let world_aabb = block_aabb.at_block(bx, by, bz);
                    if aabb.intersects_block_aabb(&world_aabb) {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Checks if moving from `old_pos` to `new_pos` would cause collision with NEW blocks.
///
/// This allows movement when already stuck in blocks (e.g., sand fell on player).
/// Only returns true if the new position collides with blocks that the old position
/// did not collide with.
///
/// Uses the physics engine's `join_is_not_empty` for proper collision detection.
///
/// Matches vanilla `ServerGamePacketListenerImpl.isEntityCollidingWithAnythingNew()`.
#[must_use]
pub fn is_colliding_with_new_blocks(world: &Arc<World>, old_pos: DVec3, new_pos: DVec3) -> bool {
    let old_aabb = make_player_aabb_deflated(old_pos);
    let new_aabb = make_player_aabb_deflated(new_pos);

    // Use physics collision provider for consistency
    let collision_world = WorldCollisionProvider::new(world);
    let collisions = collision_world.get_block_collisions(&new_aabb);

    // Check if any collision is NEW (not present at old position)
    for collision_aabb in &collisions {
        // If new position collides but old didn't, this is a NEW collision
        if join_is_not_empty(&new_aabb, collision_aabb)
            && !join_is_not_empty(&old_aabb, collision_aabb)
        {
            return true;
        }
    }

    false
}

/// Input parameters for movement validation.
#[derive(Debug, Clone)]
pub struct MovementInput {
    /// The target position the client claims to have moved to.
    pub target_pos: DVec3,
    /// The position at the start of the current tick.
    pub first_good_pos: DVec3,
    /// The last validated position.
    pub last_good_pos: DVec3,
    /// The player's current expected velocity (squared length).
    pub expected_velocity_sq: f64,
    /// Number of movement packets received since last tick.
    pub delta_packets: i32,
    /// Whether the player is using elytra.
    pub is_fall_flying: bool,
    /// Whether to skip anti-cheat checks (spectator, creative, tick frozen, gamerules).
    /// When true, all validation checks are bypassed.
    pub skip_checks: bool,
    /// Whether the player is in post-impulse grace period.
    pub in_impulse_grace: bool,
    /// Whether the player is crouching (for sneak-edge prevention).
    pub is_crouching: bool,
    /// Whether the player was on ground before this movement (affects step-up).
    pub on_ground: bool,
}

/// Result of movement validation.
#[derive(Debug, Clone)]
pub struct MovementValidation {
    /// Whether the movement is valid.
    pub is_valid: bool,
    /// The movement delta from `last_good_pos`.
    pub move_delta: DVec3,
    /// The result of physics simulation.
    pub move_result: MoveResult,
    /// Why the movement failed (if invalid).
    pub failure_reason: Option<MovementFailure>,
}

/// Reason for movement validation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementFailure {
    /// Player moved faster than allowed.
    TooFast,
    /// Client position differs too much from server simulation.
    PositionError,
    /// Player collided with new blocks.
    Collision,
}

/// Validates a player's movement.
///
/// This encapsulates the movement validation logic from vanilla's `handleMovePlayer`.
/// It runs physics simulation and checks for speed hacks, position errors, and collisions.
#[must_use]
pub fn validate_movement(world: &Arc<World>, input: &MovementInput) -> MovementValidation {
    let target_pos = input.target_pos;
    let first_good = input.first_good_pos;
    let last_good = input.last_good_pos;

    // Speed check: distance from first_good position
    let dx = target_pos.x - first_good.x;
    let dy = target_pos.y - first_good.y;
    let dz = target_pos.z - first_good.z;
    let moved_dist_sq = dx * dx + dy * dy + dz * dz;

    // Speed check
    if !input.skip_checks {
        let threshold = if input.is_fall_flying {
            SPEED_THRESHOLD_FLYING
        } else {
            SPEED_THRESHOLD_NORMAL
        } * f64::from(input.delta_packets);

        if moved_dist_sq - input.expected_velocity_sq > threshold {
            return MovementValidation {
                is_valid: false,
                move_delta: DVec3::new(0.0, 0.0, 0.0),
                move_result: MoveResult {
                    movement: DVec3::new(0.0, 0.0, 0.0),
                    position: last_good,
                    collision_x: false,
                    collision_y: false,
                    collision_z: false,
                    on_ground: false,
                },
                failure_reason: Some(MovementFailure::TooFast),
            };
        }
    }

    // Calculate movement delta from last_good position
    let move_delta = DVec3::new(
        target_pos.x - last_good.x,
        target_pos.y - last_good.y,
        target_pos.z - last_good.z,
    );

    // Run server-side physics simulation with step-up and sneak-edge
    let move_result = simulate_move(
        world,
        last_good,
        move_delta,
        input.is_crouching,
        input.on_ground,
    );

    // Calculate error between client position and server-simulated position
    let error_x = target_pos.x - move_result.position.x;
    let mut error_y = target_pos.y - move_result.position.y;
    let error_z = target_pos.z - move_result.position.z;

    // Y-axis tolerance: ignore small Y discrepancies
    if error_y > -Y_TOLERANCE && error_y < Y_TOLERANCE {
        error_y = 0.0;
    }

    let error_dist_sq = error_x * error_x + error_y * error_y + error_z * error_z;

    // Movement error check
    let error_check_failed = !input.in_impulse_grace && error_dist_sq > MOVEMENT_ERROR_THRESHOLD;

    // Collision checks
    let was_in_collision = is_in_collision(world, last_good);
    let collision_check_failed = error_check_failed
        && was_in_collision
        && is_colliding_with_new_blocks(world, last_good, target_pos);

    let new_collision_without_error =
        !error_check_failed && is_colliding_with_new_blocks(world, last_good, target_pos);

    // Determine if movement failed
    let movement_failed = !input.skip_checks
        && ((error_check_failed && !was_in_collision)
            || collision_check_failed
            || new_collision_without_error);

    if movement_failed {
        let reason = if error_check_failed && !was_in_collision {
            MovementFailure::PositionError
        } else {
            MovementFailure::Collision
        };

        return MovementValidation {
            is_valid: false,
            move_delta,
            move_result,
            failure_reason: Some(reason),
        };
    }

    MovementValidation {
        is_valid: true,
        move_delta,
        move_result,
        failure_reason: None,
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

            let (yaw, pitch) = self.rotation.load();
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

        if self.update_awaiting_teleport() {
            if packet.has_rot {
                self.rotation.store((packet.y_rot, packet.x_rot));
            }
            return;
        }

        if !self.client_loaded.load(Ordering::Relaxed) {
            return;
        }

        let (prev_pos, prev_rot) = {
            let mv = self.movement.lock();
            (mv.prev_position, mv.prev_rotation)
        };
        let start_pos = *self.position.lock();
        let game_mode = self.game_mode.load();
        let (is_sleeping, is_fall_flying, was_on_ground, is_crouching) = {
            let es = self.entity_state.lock();
            (es.sleeping, es.fall_flying, es.on_ground, es.crouching)
        };
        let is_spectator = game_mode == GameType::Spectator;
        let is_creative = game_mode == GameType::Creative;
        let world = self.get_world();
        let tick_frozen = !world.tick_runs_normally();

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
                    let (yaw, pitch) = self.rotation.load();
                    self.teleport(start_pos.x, start_pos.y, start_pos.z, yaw, pitch);
                    return;
                }
            } else {
                let mut delta_packets = {
                    let mut mv = self.movement.lock();
                    mv.received_move_packet_count += 1;
                    mv.received_move_packet_count - mv.known_move_packet_count
                };

                if delta_packets > 5 {
                    delta_packets = 1;
                }

                let gamerule_skip = !Self::should_validate_movement(&world, is_fall_flying);
                let skip_checks = is_spectator || is_creative || tick_frozen || gamerule_skip;

                let (expected_velocity_sq, in_impulse_grace) = {
                    let mv = self.movement.lock();
                    let vel_sq = mv.delta_movement_length_sq();
                    let current_tick = self.tick_count.load(Ordering::Relaxed);
                    let grace =
                        current_tick.wrapping_sub(mv.last_impulse_tick) < IMPULSE_GRACE_TICKS;
                    (vel_sq, grace)
                };

                let mut validation = validate_movement(
                    &world,
                    &MovementInput {
                        target_pos,
                        first_good_pos: first_good,
                        last_good_pos: last_good,
                        expected_velocity_sq,
                        delta_packets,
                        is_fall_flying,
                        skip_checks,
                        in_impulse_grace,
                        is_crouching,
                        on_ground: was_on_ground,
                    },
                );

                if !validation.is_valid {
                    let (yaw, pitch) = prev_rot;
                    self.teleport(start_pos.x, start_pos.y, start_pos.z, yaw, pitch);
                    return;
                }

                self.movement.lock().last_good_position = target_pos;

                if !was_on_ground && packet.on_ground {
                    validation.move_delta.y = 0.0;
                }
                self.set_delta_movement(validation.move_delta);

                let moved_upwards = validation.move_delta.y > 0.0;
                if was_on_ground && !packet.on_ground && moved_upwards {
                    if self.is_sprinting() {
                        self.cause_food_exhaustion(food_constants::EXHAUSTION_SPRINT_JUMP);
                    } else {
                        self.cause_food_exhaustion(food_constants::EXHAUSTION_JUMP);
                    }
                }

                if self.is_sprinting() {
                    let dx = validation.move_delta.x;
                    let dz = validation.move_delta.z;
                    let horizontal_dist_sq = dx * dx + dz * dz;
                    if horizontal_dist_sq > 0.0 {
                        let distance = horizontal_dist_sq.sqrt() as f32;
                        self.cause_food_exhaustion(distance * food_constants::EXHAUSTION_SPRINT);
                    }
                }
            }
        }

        self.entity_state.lock().on_ground = packet.on_ground;

        if packet.has_pos {
            let old_pos = *self.position.lock();
            *self.position.lock() = packet.position;
            self.level_callback.lock().on_move(old_pos, packet.position);
        }
        if packet.has_rot {
            self.rotation.store((packet.y_rot, packet.x_rot));
        }

        let pos = if packet.has_pos {
            packet.position
        } else {
            prev_pos
        };
        let (yaw, pitch) = if packet.has_rot {
            (packet.y_rot, packet.x_rot)
        } else {
            prev_rot
        };

        if packet.has_pos || packet.has_rot {
            let new_chunk = ChunkPos::new((pos.x as i32) >> 4, (pos.z as i32) >> 4);

            if packet.has_pos {
                let dx = calc_delta(pos.x, prev_pos.x);
                let dy = calc_delta(pos.y, prev_pos.y);
                let dz = calc_delta(pos.z, prev_pos.z);

                let (sync_delay, last_on_ground) = {
                    let mut mv = self.movement.lock();
                    let d = mv.position_sync_delay;
                    mv.position_sync_delay += 1;
                    (d, mv.last_sent_on_ground)
                };
                let on_ground_changed = last_on_ground != packet.on_ground;
                let force_sync = sync_delay > 400 || on_ground_changed;

                if let (Some(dx), Some(dy), Some(dz)) = (dx, dy, dz) {
                    if force_sync {
                        {
                            let mut mv = self.movement.lock();
                            mv.position_sync_delay = 0;
                            mv.last_sent_on_ground = packet.on_ground;
                        }

                        let delta = self.get_delta_movement();
                        let sync_packet = CEntityPositionSync {
                            entity_id: self.id,
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
                        world.broadcast_to_nearby(new_chunk, sync_packet, Some(self.id));
                    } else {
                        let move_packet = CMoveEntityPosRot {
                            entity_id: self.id,
                            dx,
                            dy,
                            dz,
                            y_rot: to_angle_byte(yaw),
                            x_rot: to_angle_byte(pitch),
                            on_ground: packet.on_ground,
                        };
                        world.broadcast_to_nearby(new_chunk, move_packet, Some(self.id));
                    }
                } else {
                    {
                        let mut mv = self.movement.lock();
                        mv.position_sync_delay = 0;
                        mv.last_sent_on_ground = packet.on_ground;
                    }

                    let delta = self.get_delta_movement();
                    let sync_packet = CEntityPositionSync {
                        entity_id: self.id,
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
                    world.broadcast_to_nearby(new_chunk, sync_packet, Some(self.id));
                }
            } else {
                let rot_packet = CMoveEntityRot {
                    entity_id: self.id,
                    y_rot: to_angle_byte(yaw),
                    x_rot: to_angle_byte(pitch),
                    on_ground: packet.on_ground,
                };
                world.broadcast_to_nearby(new_chunk, rot_packet, Some(self.id));
            }

            if packet.has_rot {
                let head_packet = CRotateHead {
                    entity_id: self.id,
                    head_y_rot: to_angle_byte(yaw),
                };
                world.broadcast_to_nearby(new_chunk, head_packet, Some(self.id));
            }

            let mut mv = self.movement.lock();
            mv.prev_position = pos;
            mv.prev_rotation = (yaw, pitch);
        }
    }

    /// Returns the player's current velocity.
    #[must_use]
    pub fn get_delta_movement(&self) -> DVec3 {
        self.movement.lock().delta_movement
    }

    /// Sets the player's velocity.
    pub fn set_delta_movement(&self, velocity: DVec3) {
        self.movement.lock().delta_movement = velocity;
    }

    /// Returns the player's current gravity value.
    ///
    /// Matches vanilla `LivingEntity.getGravity()` which reads from `Attributes.GRAVITY`.
    /// Default is 0.08 blocks/tick².
    fn get_gravity(&self) -> f64 {
        self.attributes
            .lock()
            .get_value(vanilla_attributes::GRAVITY)
            .unwrap_or(0.08)
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
        let (on_ground, is_fall_flying) = {
            let es = self.entity_state.lock();
            (es.on_ground, es.fall_flying)
        };
        let game_mode = self.game_mode.load();
        let is_spectator = game_mode == GameType::Spectator;
        let is_creative_flying = game_mode == GameType::Creative; // TODO: check actual flying state

        if on_ground || is_spectator || is_creative_flying || is_fall_flying {
            return;
        }

        let gravity = self.get_gravity();
        if gravity != 0.0 {
            self.movement.lock().delta_movement.y -= gravity;
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

        *self.position.lock() = pos;
        self.rotation.store((yaw, pitch));

        self.send_packet(CPlayerPosition::absolute(new_id, x, y, z, yaw, pitch));
    }

    /// Handles a teleport acknowledgment from the client.
    ///
    /// Matches vanilla `ServerGamePacketListenerImpl.handleAcceptTeleportPacket()`.
    pub fn handle_accept_teleportation(&self, packet: SAcceptTeleportation) {
        let mut tp = self.teleport_state.lock();

        if let Some(pos) = tp.try_accept(packet.teleport_id) {
            *self.position.lock() = pos;
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

        self.entity_state.lock().crouching = packet.shift();
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

        if packet.entity_id != self.id {
            log::warn!(
                "Player {} (eid {}) sent SPlayerCommand with mismatched entity_id {}",
                self.gameprofile.name,
                self.id,
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
                self.entity_state.lock().fall_flying = true;
            }
            PlayerCommandAction::LeaveBed => {
                let mut state = self.entity_state.lock();
                if state.sleeping {
                    state.sleeping = false;
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
    fn test_make_player_aabb() {
        let pos = DVec3::new(0.0, 64.0, 0.0);
        let aabb = make_player_aabb(pos);

        assert!((aabb.min_x - (-0.3)).abs() < 0.001);
        assert!((aabb.max_x - 0.3).abs() < 0.001);
        assert!((aabb.min_y - 64.0).abs() < 0.001);
        assert!((aabb.max_y - 65.8).abs() < 0.001);
        assert!((aabb.min_z - (-0.3)).abs() < 0.001);
        assert!((aabb.max_z - 0.3).abs() < 0.001);
    }
}
