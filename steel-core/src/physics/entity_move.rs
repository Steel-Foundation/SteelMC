//! Entity movement physics with vanilla parity.
//!
//! Implements vanilla's `Entity.move()` method with:
//! - Step-up mechanics for climbing small obstacles
//! - Sneak-edge prevention for staying on blocks while crouching
//! - Proper collision detection and resolution

use steel_registry::blocks::shapes::AABBd;
use steel_utils::math::Vector3;

use crate::physics::{
    collision::CollisionWorld, physics_state::EntityPhysicsState, shapes::collide,
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

    /// Whether horizontal collision occurred (X or Z).
    pub horizontal_collision: bool,

    /// Whether vertical collision occurred.
    pub vertical_collision: bool,

    /// Whether X-axis collision occurred (requested.x != actual.x).
    pub x_collision: bool,

    /// Whether Z-axis collision occurred (requested.z != actual.z).
    pub z_collision: bool,

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
    world: &dyn CollisionWorld,
) -> MoveResult {
    // Early exit for zero movement
    if delta.x.abs() < 1.0e-7 && delta.y.abs() < 1.0e-7 && delta.z.abs() < 1.0e-7 {
        return MoveResult {
            final_position: state.position,
            actual_movement: Vector3::new(0.0, 0.0, 0.0),
            on_ground: state.on_ground,
            horizontal_collision: false,
            vertical_collision: false,
            x_collision: false,
            z_collision: false,
            final_aabb: state.bounding_box,
        };
    }

    // Vanilla `Entity.move()` uses the real bounding box for collision.
    // Deflating here causes entities (especially items) to end up slightly inside blocks,
    // which can create client-side desync where the client thinks the entity is falling.
    let aabb = state.bounding_box;

    // Apply sneak-edge prevention if crouching and on ground
    let movement = if state.is_crouching && state.on_ground && mover_type == MoverType::SelfMovement
    {
        apply_sneak_edge_prevention(state, delta, &aabb, world)
    } else {
        delta
    };

    // Perform basic collision resolution
    let collision_result = collide_with_world(state, movement, &aabb, world);

    // Try step-up if horizontal collision occurred
    if should_try_step_up(state, &collision_result, mover_type) {
        try_step_up(state, movement, &aabb, &collision_result, world)
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
    aabb: &AABBd,
    world: &dyn CollisionWorld,
) -> Vector3<f64> {
    // Only prevent edge falling for horizontal movement
    if delta.x.abs() < 1.0e-7 && delta.z.abs() < 1.0e-7 {
        return delta;
    }

    // Calculate position after movement
    let new_aabb = AABBd {
        min_x: aabb.min_x + delta.x,
        min_y: aabb.min_y + delta.y,
        min_z: aabb.min_z + delta.z,
        max_x: aabb.max_x + delta.x,
        max_y: aabb.max_y + delta.y,
        max_z: aabb.max_z + delta.z,
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

/// Returns the axis step order for collision resolution.
///
/// Vanilla's `Direction.axisStepOrder(Vec3)` returns:
/// - YZX if `|x| < |z|` (move along Z before X)
/// - YXZ otherwise (move along X before Z)
///
/// Y is always first because gravity/vertical movement should be resolved first.
fn axis_step_order(movement: Vector3<f64>) -> [Axis; 3] {
    if movement.x.abs() < movement.z.abs() {
        [Axis::Y, Axis::Z, Axis::X]
    } else {
        [Axis::Y, Axis::X, Axis::Z]
    }
}

/// Performs collision detection and resolution along all three axes.
///
/// Matches vanilla's `Entity.collideWithShapes()` behavior exactly:
/// - Uses dynamic axis order based on movement direction (Y first, then X/Z based on magnitude)
/// - Accumulates resolved movement and moves AABB after each axis
#[allow(clippy::float_cmp)] // Intentional: checking if collision clipped the movement value
fn collide_with_world(
    state: &EntityPhysicsState,
    movement: Vector3<f64>,
    aabb: &AABBd,
    world: &dyn CollisionWorld,
) -> MoveResult {
    // Get all collision shapes that could intersect with our movement
    let swept_aabb = sweep_aabb(aabb, movement);
    let collisions = world.get_block_collisions(&swept_aabb);

    // Vanilla: collideWithShapes iterates in dynamic axis order
    let axes = axis_step_order(movement);

    // Track resolved movement per axis and current AABB position
    let mut resolved = Vector3::new(0.0, 0.0, 0.0);
    let mut current_aabb = *aabb;

    for axis in axes {
        let axis_movement = match axis {
            Axis::X => movement.x,
            Axis::Y => movement.y,
            Axis::Z => movement.z,
        };

        if axis_movement != 0.0 {
            let collision = collide(axis, &current_aabb, &collisions, axis_movement);

            // Update resolved movement for this axis
            match axis {
                Axis::X => resolved.x = collision,
                Axis::Y => resolved.y = collision,
                Axis::Z => resolved.z = collision,
            }

            // Move AABB by the resolved amount (vanilla: boundingBox.move(resolvedMovement))
            current_aabb = move_aabb(&current_aabb, axis, collision);
        }
    }

    let final_position = state.position + resolved;

    // Check if on ground (touching block below with epsilon tolerance)
    let on_ground = resolved.y != movement.y && movement.y < 0.0;

    // Detect collisions (vanilla: Entity.move lines 751-757)
    let x_collision = resolved.x != movement.x;
    let z_collision = resolved.z != movement.z;
    let horizontal_collision = x_collision || z_collision;
    let vertical_collision = resolved.y != movement.y;

    MoveResult {
        final_position,
        actual_movement: resolved,
        on_ground,
        horizontal_collision,
        vertical_collision,
        x_collision,
        z_collision,
        final_aabb: current_aabb,
    }
}

/// Moves an AABB along a single axis by the given amount.
fn move_aabb(aabb: &AABBd, axis: Axis, amount: f64) -> AABBd {
    match axis {
        Axis::X => AABBd {
            min_x: aabb.min_x + amount,
            min_y: aabb.min_y,
            min_z: aabb.min_z,
            max_x: aabb.max_x + amount,
            max_y: aabb.max_y,
            max_z: aabb.max_z,
        },
        Axis::Y => AABBd {
            min_x: aabb.min_x,
            min_y: aabb.min_y + amount,
            min_z: aabb.min_z,
            max_x: aabb.max_x,
            max_y: aabb.max_y + amount,
            max_z: aabb.max_z,
        },
        Axis::Z => AABBd {
            min_x: aabb.min_x,
            min_y: aabb.min_y,
            min_z: aabb.min_z + amount,
            max_x: aabb.max_x,
            max_y: aabb.max_y,
            max_z: aabb.max_z + amount,
        },
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
/// This implements vanilla's step-up algorithm from `Entity.collide()`.
///
/// Vanilla calls `collideWithShapes(Vec3(movement.x, stepHeight, movement.z), groundedAABB, colliders)`
/// which uses the dynamic axis order. If step-up gives more horizontal progress, use it.
///
/// Matches: `Entity.collide()` lines 1077-1095
#[allow(clippy::float_cmp)] // Intentional: checking if collision clipped the movement value
fn try_step_up(
    state: &EntityPhysicsState,
    movement: Vector3<f64>,
    aabb: &AABBd,
    ground_result: &MoveResult,
    world: &dyn CollisionWorld,
) -> MoveResult {
    let max_step = f64::from(state.max_up_step);

    // Sweep for collisions during the entire step attempt
    let step_sweep_aabb = AABBd {
        min_x: (aabb.min_x + movement.x).min(aabb.min_x),
        min_y: aabb.min_y,
        min_z: (aabb.min_z + movement.z).min(aabb.min_z),
        max_x: (aabb.max_x + movement.x).max(aabb.max_x),
        max_y: aabb.max_y + max_step,
        max_z: (aabb.max_z + movement.z).max(aabb.max_z),
    };
    let collisions = world.get_block_collisions(&step_sweep_aabb);

    // Vanilla: collideWithShapes(Vec3(movement.x, maxUpStep, movement.z), groundedAABB, colliders)
    // This uses the dynamic axis order based on movement.x vs movement.z
    let step_movement = Vector3::new(movement.x, max_step, movement.z);
    let axes = axis_step_order(step_movement);

    let mut resolved = Vector3::new(0.0, 0.0, 0.0);
    let mut current_aabb = *aabb;

    for axis in axes {
        let axis_movement = match axis {
            Axis::X => step_movement.x,
            Axis::Y => step_movement.y,
            Axis::Z => step_movement.z,
        };

        if axis_movement != 0.0 {
            let collision = collide(axis, &current_aabb, &collisions, axis_movement);

            match axis {
                Axis::X => resolved.x = collision,
                Axis::Y => resolved.y = collision,
                Axis::Z => resolved.z = collision,
            }

            current_aabb = move_aabb(&current_aabb, axis, collision);
        }
    }

    // Check if we made more horizontal progress
    let ground_dist_sq =
        ground_result.actual_movement.x.powi(2) + ground_result.actual_movement.z.powi(2);
    let step_dist_sq = resolved.x.powi(2) + resolved.z.powi(2);

    if step_dist_sq <= ground_dist_sq {
        // Step didn't help, use ground result
        return ground_result.clone();
    }

    // Vanilla subtracts the distance to ground from Y: stepFromGround.subtract(0.0, distanceToGround, 0.0)
    // distanceToGround = aabb.minY - groundedAABB.minY (0 for our setup), so the final movement is just resolved.

    let actual_movement = resolved;
    let final_position = state.position + actual_movement;

    // After stepping, we're on ground if we ended up at a position lower than max step height
    // (i.e., we landed on something)
    let on_ground = resolved.y < max_step && resolved.y > 0.0;

    let x_collision = resolved.x != movement.x;
    let z_collision = resolved.z != movement.z;

    MoveResult {
        final_position,
        actual_movement,
        on_ground,
        horizontal_collision: x_collision || z_collision,
        vertical_collision: false, // Step-up resolved the vertical collision
        x_collision,
        z_collision,
        final_aabb: current_aabb,
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
    use steel_registry::vanilla_entities;
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
        let mut state =
            EntityPhysicsState::new(Vector3::new(0.0, 10.0, 0.0), vanilla_entities::PLAYER);
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
        let mut state =
            EntityPhysicsState::new(Vector3::new(0.0, 5.0, 0.0), vanilla_entities::PLAYER);
        state.on_ground = false;

        let world = MockWorld { has_floor: true };
        let large_fall = Vector3::new(0.0, -10.0, 0.0);

        let result = move_entity(&state, large_fall, MoverType::SelfMovement, &world);

        assert!(result.on_ground, "Should be on ground after landing");

        assert!(
            result.vertical_collision,
            "Should detect vertical collision"
        );
    }

    #[test]
    fn test_move_entity_no_collision_in_air() {
        let state = EntityPhysicsState::new(Vector3::new(0.0, 10.0, 0.0), vanilla_entities::PLAYER);

        let world = MockWorld { has_floor: false };
        let movement = Vector3::new(1.0, 0.0, 1.0);

        let result = move_entity(&state, movement, MoverType::SelfMovement, &world);

        assert_eq!(
            result.actual_movement, movement,
            "Should move freely in air"
        );
        assert!(!result.horizontal_collision, "Should have no collision");
    }

    #[test]
    fn test_item_on_ground_with_accumulated_velocity() {
        // Simulates an item that's on the ground (Y=1.0 on top of floor)
        // and has accumulated negative velocity from gravity
        let mut state =
            EntityPhysicsState::new(Vector3::new(0.0, 1.0, 0.0), vanilla_entities::ITEM);
        state.on_ground = true;

        let world = MockWorld { has_floor: true };

        // Simulate accumulated velocity from 25 ticks of gravity (0.04 per tick)
        let accumulated_velocity = Vector3::new(0.0, -1.0, 0.0);

        let result = move_entity(
            &state,
            accumulated_velocity,
            MoverType::SelfMovement,
            &world,
        );

        // Item should NOT fall through the floor
        assert!(
            result.final_position.y >= 0.99,
            "Item should stay on floor, but Y = {}",
            result.final_position.y
        );
        assert!(result.on_ground, "Item should still be on ground");
    }

    #[test]
    fn test_item_slightly_above_ground() {
        // Simulates an item that's slightly above the ground due to floating point
        // Floor at Y=1.0, item at Y=1.00001 (just above)
        let mut state =
            EntityPhysicsState::new(Vector3::new(0.0, 1.00001, 0.0), vanilla_entities::ITEM);
        state.on_ground = false; // Not quite on ground yet

        let world = MockWorld { has_floor: true };

        // Small downward velocity
        let velocity = Vector3::new(0.0, -0.04, 0.0);

        let result = move_entity(&state, velocity, MoverType::SelfMovement, &world);

        // Item should land on the floor, not fall through
        assert!(
            result.final_position.y >= 0.99,
            "Item should land on floor, but Y = {}",
            result.final_position.y
        );
    }
}
