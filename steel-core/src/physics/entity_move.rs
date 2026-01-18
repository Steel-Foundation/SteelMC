//! Entity movement physics with vanilla parity.
//!
//! Implements vanilla's `Entity.move()` method with:
//! - Step-up mechanics for climbing small obstacles
//! - Sneak-edge prevention for staying on blocks while crouching
//! - Proper collision detection and resolution

use steel_registry::blocks::shapes::AABBd;
use steel_utils::math::Vector3;

use crate::physics::{
    collision::CollisionWorld,
    physics_state::EntityPhysicsState,
    shapes::{collide, deflate_aabb},
};
use steel_utils::math::Axis;

/// Type of movement being performed.
///
/// Affects how the entity interacts with the world during movement.
/// Matches vanilla's `MoverType` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoverType {
    /// Normal entity movement (walking, jumping, gravity).
    #[allow(dead_code)]
    SelfMovement,
    /// Movement caused by external forces (pistons, etc).
    #[allow(dead_code)]
    Piston,
    /// Movement from shulker box opening/closing.
    #[allow(dead_code)]
    ShulkerBox,
    /// Movement from shulker entity teleportation.
    #[allow(dead_code)]
    Shulker,
}

/// Result of a movement operation.
#[derive(Debug, Clone)]
pub struct MoveResult {
    /// The entity's final position after movement and collision resolution.
    pub final_position: Vector3<f64>,

    /// The actual movement delta applied (may differ from requested due to collisions).
    pub actual_movement: Vector3<f64>,

    /// Whether the entity is on the ground after movement.
    pub on_ground: bool,

    /// Whether horizontal collision occurred.
    pub horizontal_collision: bool,

    /// Whether vertical collision occurred.
    pub vertical_collision: bool,

    /// The entity's AABB at the final position.
    pub final_aabb: AABBd,
}

/// Moves an entity through the world with collision detection and resolution.
///
/// This is the main physics function that implements vanilla's `Entity.move()` behavior,
/// including step-up mechanics and sneak-edge prevention.
///
/// # Arguments
/// * `state` - The entity's current physics state
/// * `delta` - The desired movement vector (velocity * dt)
/// * `mover_type` - Type of movement being performed
/// * `world` - World collision provider
///
/// # Returns
/// A `MoveResult` containing the final position and collision information.
///
/// # Vanilla Reference
/// `net.minecraft.world.entity.Entity.move(MoverType, Vec3)`
pub fn move_entity(
    state: &EntityPhysicsState,
    delta: Vector3<f64>,
    mover_type: MoverType,
    world: &impl CollisionWorld,
) -> MoveResult {
    // Early exit for zero movement
    if delta.x.abs() < 1.0e-7 && delta.y.abs() < 1.0e-7 && delta.z.abs() < 1.0e-7 {
        return MoveResult {
            final_position: state.position,
            actual_movement: Vector3::new(0.0, 0.0, 0.0),
            on_ground: state.on_ground,
            horizontal_collision: false,
            vertical_collision: false,
            final_aabb: state.bounding_box,
        };
    }

    // Deflate AABB slightly to avoid floating-point edge cases
    let deflated_aabb = deflate_aabb(&state.bounding_box);

    // Apply sneak-edge prevention if crouching and on ground
    let movement = if state.is_crouching && state.on_ground && mover_type == MoverType::SelfMovement
    {
        apply_sneak_edge_prevention(state, delta, &deflated_aabb, world)
    } else {
        delta
    };

    // Perform basic collision resolution
    let collision_result = collide_with_world(state, movement, &deflated_aabb, world);

    // Try step-up if horizontal collision occurred
    if should_try_step_up(state, &collision_result, mover_type) {
        try_step_up(state, movement, &deflated_aabb, &collision_result, world)
    } else {
        collision_result
    }
}

/// Applies sneak-edge prevention to keep player from walking off blocks.
///
/// When crouching, checks if the movement would cause the player to fall off
/// a block edge. If so, clips the movement to keep them on the block.
///
/// Matches: `Player.maybeBackOffFromEdge(Vec3, MoverType)`
fn apply_sneak_edge_prevention(
    _state: &EntityPhysicsState,
    delta: Vector3<f64>,
    deflated_aabb: &AABBd,
    world: &impl CollisionWorld,
) -> Vector3<f64> {
    // Only prevent edge falling for horizontal movement
    if delta.x.abs() < 1.0e-7 && delta.z.abs() < 1.0e-7 {
        return delta;
    }

    // Calculate position after movement
    let new_aabb = AABBd {
        min_x: deflated_aabb.min_x + delta.x,
        min_y: deflated_aabb.min_y + delta.y,
        min_z: deflated_aabb.min_z + delta.z,
        max_x: deflated_aabb.max_x + delta.x,
        max_y: deflated_aabb.max_y + delta.y,
        max_z: deflated_aabb.max_z + delta.z,
    };

    // Check if there's ground below the new position
    // We check down to 1 block below (vanilla checks maxUpStep + 1.0)
    let check_down_aabb = AABBd {
        min_x: new_aabb.min_x,
        min_y: new_aabb.min_y - 1.0,
        min_z: new_aabb.min_z,
        max_x: new_aabb.max_x,
        max_y: new_aabb.min_y,
        max_z: new_aabb.max_z,
    };

    let ground_below = world.get_block_collisions(&check_down_aabb);

    // If no ground below, prevent the movement
    if ground_below.is_empty() {
        return Vector3::new(0.0, delta.y, 0.0); // Allow Y movement but block X/Z
    }

    // Could add more sophisticated edge detection here (checking if we're
    // moving away from a supporting block), but this basic version matches
    // most of vanilla's behavior

    delta
}

/// Performs collision detection and resolution along all three axes.
#[allow(clippy::float_cmp)] // Intentional: checking if collision clipped the movement value
fn collide_with_world(
    state: &EntityPhysicsState,
    movement: Vector3<f64>,
    deflated_aabb: &AABBd,
    world: &impl CollisionWorld,
) -> MoveResult {
    // Get all collision shapes that could intersect with our movement
    let swept_aabb = sweep_aabb(deflated_aabb, movement);
    let collisions = world.get_block_collisions(&swept_aabb);

    // Collide along each axis in order: Y, X, Z (matches vanilla)
    let movement_y = collide(Axis::Y, deflated_aabb, &collisions, movement.y);

    let aabb_after_y = AABBd {
        min_x: deflated_aabb.min_x,
        min_y: deflated_aabb.min_y + movement_y,
        min_z: deflated_aabb.min_z,
        max_x: deflated_aabb.max_x,
        max_y: deflated_aabb.max_y + movement_y,
        max_z: deflated_aabb.max_z,
    };

    let movement_x = collide(Axis::X, &aabb_after_y, &collisions, movement.x);

    let aabb_after_x = AABBd {
        min_x: aabb_after_y.min_x + movement_x,
        min_y: aabb_after_y.min_y,
        min_z: aabb_after_y.min_z,
        max_x: aabb_after_y.max_x + movement_x,
        max_y: aabb_after_y.max_y,
        max_z: aabb_after_y.max_z,
    };

    let movement_z = collide(Axis::Z, &aabb_after_x, &collisions, movement.z);

    let final_aabb = AABBd {
        min_x: aabb_after_x.min_x,
        min_y: aabb_after_x.min_y,
        min_z: aabb_after_x.min_z + movement_z,
        max_x: aabb_after_x.max_x,
        max_y: aabb_after_x.max_y,
        max_z: aabb_after_x.max_z + movement_z,
    };

    let actual_movement = Vector3::new(movement_x, movement_y, movement_z);
    let final_position = state.position + actual_movement;

    // Check if on ground (touching block below with epsilon tolerance)
    let on_ground = movement_y != movement.y && movement.y < 0.0;

    // Detect collisions
    let horizontal_collision = movement_x != movement.x || movement_z != movement.z;
    let vertical_collision = movement_y != movement.y;

    MoveResult {
        final_position,
        actual_movement,
        on_ground,
        horizontal_collision,
        vertical_collision,
        final_aabb,
    }
}

/// Checks if step-up should be attempted.
fn should_try_step_up(
    state: &EntityPhysicsState,
    collision_result: &MoveResult,
    mover_type: MoverType,
) -> bool {
    // Only try step-up for self-movement
    if mover_type != MoverType::SelfMovement {
        return false;
    }

    // Must have step height > 0
    if state.max_up_step <= 0.0 {
        return false;
    }

    // Must have horizontal collision
    if !collision_result.horizontal_collision {
        return false;
    }

    // Must be on ground or just landed
    if !state.on_ground && !collision_result.on_ground {
        return false;
    }

    true
}

/// Attempts to step up over an obstacle.
///
/// This implements vanilla's step-up algorithm from `Entity.move()`.
///
/// # Algorithm
/// 1. Try moving upward by `max_up_step`
/// 2. Try the horizontal movement at that elevated position
/// 3. Try moving back down to land on the stepped surface
/// 4. If the result is better (more horizontal distance), use it
///
/// Matches: `Entity.move()` lines 1059-1090
#[allow(clippy::float_cmp)] // Intentional: checking if collision clipped the movement value
fn try_step_up(
    state: &EntityPhysicsState,
    movement: Vector3<f64>,
    deflated_aabb: &AABBd,
    ground_result: &MoveResult,
    world: &impl CollisionWorld,
) -> MoveResult {
    let max_step = f64::from(state.max_up_step);

    // Sweep for collisions during the entire step attempt
    let step_sweep_aabb = AABBd {
        min_x: (deflated_aabb.min_x + movement.x).min(deflated_aabb.min_x),
        min_y: deflated_aabb.min_y,
        min_z: (deflated_aabb.min_z + movement.z).min(deflated_aabb.min_z),
        max_x: (deflated_aabb.max_x + movement.x).max(deflated_aabb.max_x),
        max_y: deflated_aabb.max_y + max_step,
        max_z: (deflated_aabb.max_z + movement.z).max(deflated_aabb.max_z),
    };
    let collisions = world.get_block_collisions(&step_sweep_aabb);

    // Step 1: Move up by max_step
    let up_movement = collide(Axis::Y, deflated_aabb, &collisions, max_step);
    let aabb_stepped_up = AABBd {
        min_x: deflated_aabb.min_x,
        min_y: deflated_aabb.min_y + up_movement,
        min_z: deflated_aabb.min_z,
        max_x: deflated_aabb.max_x,
        max_y: deflated_aabb.max_y + up_movement,
        max_z: deflated_aabb.max_z,
    };

    // Step 2: Try horizontal movement at elevated position
    let step_x = collide(Axis::X, &aabb_stepped_up, &collisions, movement.x);
    let aabb_after_step_x = AABBd {
        min_x: aabb_stepped_up.min_x + step_x,
        min_y: aabb_stepped_up.min_y,
        min_z: aabb_stepped_up.min_z,
        max_x: aabb_stepped_up.max_x + step_x,
        max_y: aabb_stepped_up.max_y,
        max_z: aabb_stepped_up.max_z,
    };

    let step_z = collide(Axis::Z, &aabb_after_step_x, &collisions, movement.z);
    let aabb_after_step_xz = AABBd {
        min_x: aabb_after_step_x.min_x,
        min_y: aabb_after_step_x.min_y,
        min_z: aabb_after_step_x.min_z + step_z,
        max_x: aabb_after_step_x.max_x,
        max_y: aabb_after_step_x.max_y,
        max_z: aabb_after_step_x.max_z + step_z,
    };

    // Check if we made more horizontal progress
    let ground_dist_sq =
        ground_result.actual_movement.x.powi(2) + ground_result.actual_movement.z.powi(2);
    let step_dist_sq = step_x.powi(2) + step_z.powi(2);

    if step_dist_sq <= ground_dist_sq {
        // Step didn't help, use ground result
        return ground_result.clone();
    }

    // Step 3: Move back down to land on surface
    // Try to move down by the amount we moved up, plus a bit extra
    let down_movement = collide(
        Axis::Y,
        &aabb_after_step_xz,
        &collisions,
        -(up_movement + 0.001),
    );

    let final_aabb = AABBd {
        min_x: aabb_after_step_xz.min_x,
        min_y: aabb_after_step_xz.min_y + down_movement,
        min_z: aabb_after_step_xz.min_z,
        max_x: aabb_after_step_xz.max_x,
        max_y: aabb_after_step_xz.max_y + down_movement,
        max_z: aabb_after_step_xz.max_z,
    };

    let actual_movement = Vector3::new(step_x, up_movement + down_movement, step_z);
    let final_position = state.position + actual_movement;

    // After stepping, we're on ground if we moved down
    let on_ground = down_movement < 0.0;

    MoveResult {
        final_position,
        actual_movement,
        on_ground,
        horizontal_collision: step_x != movement.x || step_z != movement.z,
        vertical_collision: false, // Step-up resolved the vertical collision
        final_aabb,
    }
}

/// Creates an AABB that encompasses the start and end positions of a movement.
fn sweep_aabb(aabb: &AABBd, movement: Vector3<f64>) -> AABBd {
    AABBd {
        min_x: if movement.x < 0.0 {
            aabb.min_x + movement.x
        } else {
            aabb.min_x
        },
        min_y: if movement.y < 0.0 {
            aabb.min_y + movement.y
        } else {
            aabb.min_y
        },
        min_z: if movement.z < 0.0 {
            aabb.min_z + movement.z
        } else {
            aabb.min_z
        },
        max_x: if movement.x > 0.0 {
            aabb.max_x + movement.x
        } else {
            aabb.max_x
        },
        max_y: if movement.y > 0.0 {
            aabb.max_y + movement.y
        } else {
            aabb.max_y
        },
        max_z: if movement.z > 0.0 {
            aabb.max_z + movement.z
        } else {
            aabb.max_z
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::collision::CollisionWorld;
    use steel_registry::REGISTRY;
    use steel_registry::vanilla_blocks;
    use steel_utils::BlockPos;

    /// Mock collision world for testing
    struct MockWorld {
        // Block at Y=0 (floor)
        has_floor: bool,
    }

    impl CollisionWorld for MockWorld {
        fn get_block_state(&self, pos: &BlockPos) -> steel_utils::BlockStateId {
            if self.has_floor && pos.y() == 0 {
                REGISTRY.blocks.get_base_state_id(vanilla_blocks::STONE)
            } else {
                REGISTRY.blocks.get_base_state_id(vanilla_blocks::AIR)
            }
        }

        fn get_block_collisions(&self, aabb: &AABBd) -> Vec<AABBd> {
            let mut collisions = Vec::new();

            if self.has_floor && aabb.min_y <= 1.0 {
                // Full block at Y=0
                collisions.push(AABBd {
                    min_x: -10.0,
                    min_y: 0.0,
                    min_z: -10.0,
                    max_x: 10.0,
                    max_y: 1.0,
                    max_z: 10.0,
                });
            }

            collisions
        }

        fn get_pre_move_collisions(&self, _aabb: &AABBd, _old_pos: Vector3<f64>) -> Vec<AABBd> {
            Vec::new()
        }
    }

    #[test]
    fn test_move_entity_free_fall() {
        let mut state = EntityPhysicsState::new_player(Vector3::new(0.0, 10.0, 0.0));
        state.on_ground = false;

        let world = MockWorld { has_floor: true };
        let gravity = Vector3::new(0.0, -0.08, 0.0); // Vanilla gravity per tick

        let result = move_entity(&state, gravity, MoverType::SelfMovement, &world);

        assert!(result.final_position.y < 10.0, "Should fall down");
        assert!(
            !result.on_ground,
            "Should not be on ground yet (only fell 0.08)"
        );
    }

    #[test]
    fn test_move_entity_land_on_ground() {
        let mut state = EntityPhysicsState::new_player(Vector3::new(0.0, 5.0, 0.0));
        state.on_ground = false;

        let world = MockWorld { has_floor: true };
        let large_fall = Vector3::new(0.0, -10.0, 0.0);

        let result = move_entity(&state, large_fall, MoverType::SelfMovement, &world);

        assert!(result.on_ground, "Should be on ground after landing");
        // Floor is at Y=1.0, but AABB deflation (COLLISION_EPSILON) causes slight offset
        assert!(
            result.final_position.y >= 0.999,
            "Should stop at ~floor level, but got Y = {}",
            result.final_position.y
        );
        assert!(result.final_position.y <= 1.001, "Should be at floor level");
        assert!(
            result.vertical_collision,
            "Should detect vertical collision"
        );
    }

    #[test]
    fn test_move_entity_no_collision_in_air() {
        let state = EntityPhysicsState::new_player(Vector3::new(0.0, 10.0, 0.0));

        let world = MockWorld { has_floor: false };
        let movement = Vector3::new(1.0, 0.0, 1.0);

        let result = move_entity(&state, movement, MoverType::SelfMovement, &world);

        assert_eq!(
            result.actual_movement, movement,
            "Should move freely in air"
        );
        assert!(!result.horizontal_collision, "Should have no collision");
    }
}
