//! Player movement physics and validation.
//!
//! This module handles server-side movement simulation and anti-cheat checks.
//! It implements collision detection and physics similar to vanilla Minecraft.

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::shapes::AABBd;
use steel_registry::vanilla_entities;
use steel_utils::{BlockPos, math::Vector3};

use crate::physics::{
    CollisionWorld, EntityPhysicsState, MoverType, WorldCollisionProvider, join_is_not_empty,
    move_entity,
};
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
pub fn make_player_aabb(pos: Vector3<f64>) -> AABBd {
    AABBd::entity_box(pos.x, pos.y, pos.z, PLAYER_WIDTH / 2.0, PLAYER_HEIGHT)
}

/// Creates a player bounding box at the given position, deflated by the collision epsilon.
#[must_use]
pub fn make_player_aabb_deflated(pos: Vector3<f64>) -> AABBd {
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
    pub movement: Vector3<f64>,
    /// The final position after movement.
    pub position: Vector3<f64>,
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
    world: &World,
    start_pos: Vector3<f64>,
    delta: Vector3<f64>,
    is_crouching: bool,
    on_ground: bool,
) -> MoveResult {
    // Create physics state for the player
    let mut state = EntityPhysicsState::new(start_pos, vanilla_entities::PLAYER);
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
pub fn is_in_collision(world: &World, pos: Vector3<f64>) -> bool {
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
                let block_state = world.get_block_state(&block_pos);
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
pub fn is_colliding_with_new_blocks(
    world: &World,
    old_pos: Vector3<f64>,
    new_pos: Vector3<f64>,
) -> bool {
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
    pub target_pos: Vector3<f64>,
    /// The position at the start of the current tick.
    pub first_good_pos: Vector3<f64>,
    /// The last validated position.
    pub last_good_pos: Vector3<f64>,
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
    pub move_delta: Vector3<f64>,
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
pub fn validate_movement(world: &World, input: &MovementInput) -> MovementValidation {
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
                move_delta: Vector3::new(0.0, 0.0, 0.0),
                move_result: MoveResult {
                    movement: Vector3::new(0.0, 0.0, 0.0),
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
    let move_delta = Vector3::new(
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

#[cfg(test)]
#[allow(clippy::float_cmp)]
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
        let pos = Vector3::new(0.0, 64.0, 0.0);
        let aabb = make_player_aabb(pos);

        assert!((aabb.min_x - (-0.3)).abs() < 0.001);
        assert!((aabb.max_x - 0.3).abs() < 0.001);
        assert!((aabb.min_y - 64.0).abs() < 0.001);
        assert!((aabb.max_y - 65.8).abs() < 0.001);
        assert!((aabb.min_z - (-0.3)).abs() < 0.001);
        assert!((aabb.max_z - 0.3).abs() < 0.001);
    }
}
