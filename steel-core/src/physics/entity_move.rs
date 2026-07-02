//! Entity movement physics with vanilla parity.
//!
//! Implements vanilla's `Entity.move()` method with:
//! - Step-up mechanics for climbing small obstacles
//! - Sneak-edge prevention for staying on blocks while crouching
//! - Proper collision detection and resolution

use glam::DVec3;
use steel_utils::WorldAabb;

use crate::behavior::BlockCollisionContext;
use crate::physics::{
    collision::CollisionWorld, physics_state::EntityPhysicsState, shapes::collide,
};
use steel_utils::axis::Axis;

const ZERO_MOVEMENT_EPSILON: f64 = 1.0e-7;
const EDGE_STEP: f64 = 0.05;
const EDGE_COLLISION_EPSILON: f64 = 1.0e-7;
const STEP_HEIGHT_COLLISION_EPSILON: f64 = 1.0e-5;
const MTH_EQUAL_EPSILON: f64 = 1.0e-5;

/// Type of movement being performed.
///
/// Affects how the entity interacts with the world during movement.
/// Matches vanilla's `MoverType` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoverType {
    /// Normal entity movement (walking, jumping, gravity).
    SelfMovement,
    /// Movement requested by a serverbound player or controlled-vehicle packet.
    Player,
    /// Movement caused by external forces (pistons, etc).
    Piston,
    /// Movement from shulker box opening/closing.
    ShulkerBox,
    /// Movement from shulker entity teleportation.
    Shulker,
}

/// Result of a movement operation.
#[derive(Debug, Clone)]
pub struct MoveResult {
    /// The entity's final position after movement and collision resolution.
    pub final_position: DVec3,

    /// The actual movement delta applied (may differ from requested due to collisions).
    pub actual_movement: DVec3,

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
    pub final_aabb: WorldAabb,
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
pub(crate) fn move_entity(
    state: &EntityPhysicsState,
    delta: DVec3,
    mover_type: MoverType,
    world: &dyn CollisionWorld,
) -> MoveResult {
    // Early exit for zero movement
    if delta.x.abs() < ZERO_MOVEMENT_EPSILON
        && delta.y.abs() < ZERO_MOVEMENT_EPSILON
        && delta.z.abs() < ZERO_MOVEMENT_EPSILON
    {
        return MoveResult {
            final_position: state.position(),
            actual_movement: DVec3::new(0.0, 0.0, 0.0),
            on_ground: state.on_ground(),
            horizontal_collision: false,
            vertical_collision: false,
            x_collision: false,
            z_collision: false,
            final_aabb: state.bounding_box(),
        };
    }

    // Vanilla `Entity.move()` uses the real bounding box for collision.
    // Deflating here causes entities (especially items) to end up slightly inside blocks,
    // which can create client-side desync where the client thinks the entity is falling.
    let aabb = state.bounding_box();

    // Apply sneak-edge prevention if crouching.
    let movement = if state.backs_off_from_edge()
        && matches!(mover_type, MoverType::SelfMovement | MoverType::Player)
    {
        apply_sneak_edge_prevention(state, delta, &aabb, world)
    } else {
        delta
    };

    let swept_aabb = sweep_aabb(&aabb, movement);
    let entity_collisions = world.get_entity_collisions(&swept_aabb);

    // Perform basic collision resolution
    let collision_result = collide_with_world(state, movement, &aabb, world, &entity_collisions);

    // Try step-up if horizontal collision occurred
    if should_try_step_up(state, &collision_result, mover_type) {
        try_step_up(
            state,
            movement,
            &aabb,
            &collision_result,
            &entity_collisions,
            world,
        )
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
    state: &EntityPhysicsState,
    delta: DVec3,
    aabb: &WorldAabb,
    world: &dyn CollisionWorld,
) -> DVec3 {
    if delta.y > 0.0 || !is_above_ground(state, aabb, world) {
        return delta;
    }

    let max_down_step = f64::from(state.max_up_step());
    let mut delta_x = delta.x;
    let mut delta_z = delta.z;
    let step_x = delta_x.signum() * EDGE_STEP;
    let step_z = delta_z.signum() * EDGE_STEP;

    while delta_x != 0.0 && can_fall_at_least(state, aabb, delta_x, 0.0, max_down_step, world) {
        if delta_x.abs() <= EDGE_STEP {
            delta_x = 0.0;
            break;
        }

        delta_x -= step_x;
    }

    while delta_z != 0.0 && can_fall_at_least(state, aabb, 0.0, delta_z, max_down_step, world) {
        if delta_z.abs() <= EDGE_STEP {
            delta_z = 0.0;
            break;
        }

        delta_z -= step_z;
    }

    while delta_x != 0.0
        && delta_z != 0.0
        && can_fall_at_least(state, aabb, delta_x, delta_z, max_down_step, world)
    {
        if delta_x.abs() <= EDGE_STEP {
            delta_x = 0.0;
        } else {
            delta_x -= step_x;
        }

        if delta_z.abs() <= EDGE_STEP {
            delta_z = 0.0;
        } else {
            delta_z -= step_z;
        }
    }

    DVec3::new(delta_x, delta.y, delta_z)
}

fn is_above_ground(
    state: &EntityPhysicsState,
    aabb: &WorldAabb,
    world: &dyn CollisionWorld,
) -> bool {
    if state.on_ground() {
        return true;
    }

    let max_down_step = f64::from(state.max_up_step());
    let fall_distance = state.fall_distance();
    fall_distance < max_down_step
        && !can_fall_at_least(state, aabb, 0.0, 0.0, max_down_step - fall_distance, world)
}

fn can_fall_at_least(
    state: &EntityPhysicsState,
    aabb: &WorldAabb,
    delta_x: f64,
    delta_z: f64,
    min_height: f64,
    world: &dyn CollisionWorld,
) -> bool {
    if min_height <= 0.0 {
        return false;
    }

    let fall_aabb = WorldAabb::new(
        aabb.min_x() + EDGE_COLLISION_EPSILON + delta_x,
        aabb.min_y() - min_height - EDGE_COLLISION_EPSILON,
        aabb.min_z() + EDGE_COLLISION_EPSILON + delta_z,
        aabb.max_x() - EDGE_COLLISION_EPSILON + delta_x,
        aabb.min_y(),
        aabb.max_z() - EDGE_COLLISION_EPSILON + delta_z,
    );

    !world.has_collision_with_context(&fall_aabb, state.block_collision_context())
}

/// Returns the axis step order for collision resolution.
///
/// Vanilla's `Direction.axisStepOrder(Vec3)` returns:
/// - YZX if `|x| < |z|` (move along Z before X)
/// - YXZ otherwise (move along X before Z)
///
/// Y is always first because gravity/vertical movement should be resolved first.
fn axis_step_order(movement: DVec3) -> [Axis; 3] {
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
#[expect(
    clippy::float_cmp,
    reason = "intentional: checking if collision clipped the movement value"
)]
fn collide_with_world(
    state: &EntityPhysicsState,
    movement: DVec3,
    aabb: &WorldAabb,
    world: &dyn CollisionWorld,
    entity_collisions: &[WorldAabb],
) -> MoveResult {
    // Get all collision shapes that could intersect with our movement
    let swept_aabb = sweep_aabb(aabb, movement);
    let collisions = collect_collisions_with_context(
        world,
        &swept_aabb,
        state.block_collision_context(),
        entity_collisions,
    );

    let (resolved, current_aabb) = collide_with_shapes(movement, aabb, &collisions);
    let final_position = state.position() + resolved;

    // Check if on ground (touching block below with epsilon tolerance)
    let on_ground = resolved.y != movement.y && movement.y < 0.0;

    // Detect collisions (vanilla: Entity.move lines 751-757)
    let x_collision = horizontal_axis_collided(movement.x, resolved.x);
    let z_collision = horizontal_axis_collided(movement.z, resolved.z);
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

/// Resolves movement against a pre-collected shape set.
fn collide_with_shapes(
    movement: DVec3,
    aabb: &WorldAabb,
    collisions: &[WorldAabb],
) -> (DVec3, WorldAabb) {
    // Vanilla: collideWithShapes iterates in dynamic axis order
    let axes = axis_step_order(movement);

    // Track resolved movement per axis and current AABB position
    let mut resolved = DVec3::new(0.0, 0.0, 0.0);
    let mut current_aabb = *aabb;

    for axis in axes {
        let axis_movement = match axis {
            Axis::X => movement.x,
            Axis::Y => movement.y,
            Axis::Z => movement.z,
        };

        if axis_movement != 0.0 {
            let collision = collide(axis, &current_aabb, collisions, axis_movement);

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

    (resolved, current_aabb)
}

fn collect_collisions_with_context(
    world: &dyn CollisionWorld,
    aabb: &WorldAabb,
    context: BlockCollisionContext,
    entity_collisions: &[WorldAabb],
) -> Vec<WorldAabb> {
    let mut collisions = Vec::with_capacity(entity_collisions.len());
    collisions.extend_from_slice(entity_collisions);
    collisions.extend(world.get_world_border_collisions(aabb));
    collisions.extend(world.get_block_collisions_with_context(aabb, context));
    collisions
}

/// Moves an AABB along a single axis by the given amount.
fn move_aabb(aabb: &WorldAabb, axis: Axis, amount: f64) -> WorldAabb {
    match axis {
        Axis::X => aabb.translate(DVec3::ZERO.with_x(amount)),
        Axis::Y => aabb.translate(DVec3::ZERO.with_y(amount)),
        Axis::Z => aabb.translate(DVec3::ZERO.with_z(amount)),
    }
}

fn horizontal_axis_collided(requested: f64, actual: f64) -> bool {
    // Vanilla reports horizontal collision with `!Mth.equal(requested, actual)`.
    (actual - requested).abs() >= MTH_EQUAL_EPSILON
}

/// Checks if step-up should be attempted.
fn should_try_step_up(
    state: &EntityPhysicsState,
    collision_result: &MoveResult,
    mover_type: MoverType,
) -> bool {
    // Only try step-up for normal entity/player movement.
    if !matches!(mover_type, MoverType::SelfMovement | MoverType::Player) {
        return false;
    }

    // Must have step height > 0
    if state.max_up_step() <= 0.0 {
        return false;
    }

    // Must have horizontal collision
    if !collision_result.horizontal_collision {
        return false;
    }

    // Must be on ground or just landed
    if !state.on_ground() && !collision_result.on_ground {
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
#[expect(
    clippy::float_cmp,
    reason = "intentional: checking if collision clipped the movement value"
)]
fn try_step_up(
    state: &EntityPhysicsState,
    movement: DVec3,
    aabb: &WorldAabb,
    ground_result: &MoveResult,
    entity_collisions: &[WorldAabb],
    world: &dyn CollisionWorld,
) -> MoveResult {
    let max_step = f64::from(state.max_up_step());
    let on_ground_after_collision = ground_result.vertical_collision && movement.y < 0.0;
    let grounded_aabb = if on_ground_after_collision {
        aabb.translate(DVec3::ZERO.with_y(ground_result.actual_movement.y))
    } else {
        *aabb
    };

    let mut step_sweep_aabb =
        grounded_aabb.expand_towards(DVec3::new(movement.x, max_step, movement.z));
    if !on_ground_after_collision {
        step_sweep_aabb =
            step_sweep_aabb.expand_towards(DVec3::new(0.0, -STEP_HEIGHT_COLLISION_EPSILON, 0.0));
    }
    let collisions = collect_collisions_with_context(
        world,
        &step_sweep_aabb,
        state.block_collision_context(),
        entity_collisions,
    );
    let candidates = collect_candidate_step_up_heights(
        &grounded_aabb,
        &collisions,
        max_step,
        ground_result.actual_movement.y,
    );

    let ground_dist_sq =
        ground_result.actual_movement.x.powi(2) + ground_result.actual_movement.z.powi(2);

    for candidate in candidates {
        let step_movement = DVec3::new(movement.x, candidate, movement.z);
        let (step_from_ground, stepped_aabb) =
            collide_with_shapes(step_movement, &grounded_aabb, &collisions);
        let step_dist_sq = step_from_ground.x.powi(2) + step_from_ground.z.powi(2);

        if step_dist_sq <= ground_dist_sq {
            continue;
        }

        let distance_to_ground = aabb.min_y() - grounded_aabb.min_y();
        let actual_movement = step_from_ground - DVec3::new(0.0, distance_to_ground, 0.0);
        let final_aabb = stepped_aabb.translate(DVec3::ZERO.with_y(-distance_to_ground));
        let x_collision = horizontal_axis_collided(movement.x, actual_movement.x);
        let z_collision = horizontal_axis_collided(movement.z, actual_movement.z);
        let vertical_collision = actual_movement.y != movement.y;

        return MoveResult {
            final_position: state.position() + actual_movement,
            actual_movement,
            on_ground: vertical_collision && movement.y < 0.0,
            horizontal_collision: x_collision || z_collision,
            vertical_collision,
            x_collision,
            z_collision,
            final_aabb,
        };
    }

    MoveResult {
        final_position: ground_result.final_position,
        actual_movement: ground_result.actual_movement,
        on_ground: ground_result.on_ground,
        horizontal_collision: ground_result.horizontal_collision,
        vertical_collision: ground_result.vertical_collision,
        x_collision: ground_result.x_collision,
        z_collision: ground_result.z_collision,
        final_aabb: ground_result.final_aabb,
    }
}

#[expect(
    clippy::float_cmp,
    reason = "intentional: vanilla candidate filtering uses exact float equality"
)]
fn collect_candidate_step_up_heights(
    grounded_aabb: &WorldAabb,
    collisions: &[WorldAabb],
    max_step_height: f64,
    step_height_to_skip: f64,
) -> Vec<f64> {
    let mut candidates = Vec::new();

    for collider in collisions {
        push_step_height_candidate(
            &mut candidates,
            collider.min_y() - grounded_aabb.min_y(),
            max_step_height,
            step_height_to_skip,
        );
        push_step_height_candidate(
            &mut candidates,
            collider.max_y() - grounded_aabb.min_y(),
            max_step_height,
            step_height_to_skip,
        );
    }

    candidates.sort_by(f64::total_cmp);
    candidates.dedup_by(|a, b| *a == *b);
    candidates
}

#[expect(
    clippy::float_cmp,
    reason = "intentional: vanilla candidate filtering uses exact float equality"
)]
fn push_step_height_candidate(
    candidates: &mut Vec<f64>,
    relative_height: f64,
    max_step_height: f64,
    step_height_to_skip: f64,
) {
    if relative_height < 0.0
        || relative_height > max_step_height
        || relative_height == step_height_to_skip
    {
        return;
    }

    candidates.push(relative_height);
}

/// Creates an AABB that encompasses the start and end positions of a movement.
fn sweep_aabb(aabb: &WorldAabb, movement: DVec3) -> WorldAabb {
    aabb.expand_towards(movement)
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
        fn get_block_state(&self, pos: BlockPos) -> steel_utils::BlockStateId {
            if self.has_floor && pos.y() == 0 {
                REGISTRY.blocks.get_base_state_id(&vanilla_blocks::STONE)
            } else {
                REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR)
            }
        }

        fn get_block_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
            let mut collisions = Vec::new();

            if self.has_floor && aabb.min_y() <= 1.0 {
                // Full block at Y=0
                collisions.push(WorldAabb::new(-10.0, 0.0, -10.0, 10.0, 1.0, 10.0));
            }

            collisions
        }

        fn get_pre_move_collisions(
            &self,
            _aabb: &WorldAabb,
            _old_pos: DVec3,
            _descending: bool,
        ) -> Vec<WorldAabb> {
            Vec::new()
        }
    }

    struct BoxWorld {
        boxes: Vec<WorldAabb>,
    }

    impl CollisionWorld for BoxWorld {
        fn get_block_state(&self, _pos: BlockPos) -> steel_utils::BlockStateId {
            REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR)
        }

        fn get_block_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
            self.boxes
                .iter()
                .copied()
                .filter(|collision| collision.intersects(*aabb))
                .collect()
        }

        fn get_pre_move_collisions(
            &self,
            _aabb: &WorldAabb,
            _old_pos: DVec3,
            _descending: bool,
        ) -> Vec<WorldAabb> {
            Vec::new()
        }
    }

    struct EntityBoxWorld {
        boxes: Vec<WorldAabb>,
    }

    impl CollisionWorld for EntityBoxWorld {
        fn get_block_state(&self, _pos: BlockPos) -> steel_utils::BlockStateId {
            REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR)
        }

        fn get_block_collisions(&self, _aabb: &WorldAabb) -> Vec<WorldAabb> {
            Vec::new()
        }

        fn get_entity_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
            self.boxes
                .iter()
                .copied()
                .filter(|collision| collision.intersects(*aabb))
                .collect()
        }
    }

    struct BorderWorld {
        boxes: Vec<WorldAabb>,
    }

    impl CollisionWorld for BorderWorld {
        fn get_block_state(&self, _pos: BlockPos) -> steel_utils::BlockStateId {
            REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR)
        }

        fn get_block_collisions(&self, _aabb: &WorldAabb) -> Vec<WorldAabb> {
            Vec::new()
        }

        fn get_world_border_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
            self.boxes
                .iter()
                .copied()
                .filter(|collision| collision.intersects(*aabb))
                .collect()
        }
    }

    fn player_state(position: DVec3) -> EntityPhysicsState {
        EntityPhysicsState::with_dimensions(position, vanilla_entities::PLAYER.dimensions, 0.6)
    }

    fn item_state(position: DVec3) -> EntityPhysicsState {
        EntityPhysicsState::with_dimensions(position, vanilla_entities::ITEM.dimensions, 0.6)
    }

    #[test]
    fn test_move_entity_free_fall() {
        let state = player_state(DVec3::new(0.0, 10.0, 0.0));

        let world = MockWorld { has_floor: true };
        let gravity = DVec3::new(0.0, -0.08, 0.0); // Vanilla gravity per tick

        let result = move_entity(&state, gravity, MoverType::SelfMovement, &world);

        assert!(result.final_position.y < 10.0, "Should fall down");
        assert!(
            !result.on_ground,
            "Should not be on ground yet (only fell 0.08)"
        );
    }

    #[test]
    fn test_move_entity_land_on_ground() {
        let state = player_state(DVec3::new(0.0, 5.0, 0.0));

        let world = MockWorld { has_floor: true };
        let large_fall = DVec3::new(0.0, -10.0, 0.0);

        let result = move_entity(&state, large_fall, MoverType::SelfMovement, &world);

        assert!(result.on_ground, "Should be on ground after landing");

        assert!(
            result.vertical_collision,
            "Should detect vertical collision"
        );
    }

    #[test]
    fn test_move_entity_no_collision_in_air() {
        let state = player_state(DVec3::new(0.0, 10.0, 0.0));

        let world = MockWorld { has_floor: false };
        let movement = DVec3::new(1.0, 0.0, 1.0);

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
        let state = item_state(DVec3::new(0.0, 1.0, 0.0)).with_on_ground(true);

        let world = MockWorld { has_floor: true };

        // Simulate accumulated velocity from 25 ticks of gravity (0.04 per tick)
        let accumulated_velocity = DVec3::new(0.0, -1.0, 0.0);

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
        let state = item_state(DVec3::new(0.0, 1.00001, 0.0));

        let world = MockWorld { has_floor: true };

        // Small downward velocity
        let velocity = DVec3::new(0.0, -0.04, 0.0);

        let result = move_entity(&state, velocity, MoverType::SelfMovement, &world);

        // Item should land on the floor, not fall through
        assert!(
            result.final_position.y >= 0.99,
            "Item should land on floor, but Y = {}",
            result.final_position.y
        );
    }

    #[test]
    fn test_crouching_backs_off_from_edge_incrementally() {
        let state = player_state(DVec3::new(0.0, 1.0, 0.0))
            .with_on_ground(true)
            .with_backs_off_from_edge(true);

        let world = BoxWorld {
            boxes: vec![WorldAabb::new(-2.0, 0.0, -2.0, 0.5, 1.0, 2.0)],
        };

        let result = move_entity(&state, DVec3::new(1.0, 0.0, 0.0), MoverType::Player, &world);

        assert!(
            result.actual_movement.x > 0.0 && result.actual_movement.x < 1.0,
            "sneak edge should trim movement instead of fully allowing or fully blocking it: {:?}",
            result.actual_movement
        );
        assert!(result.actual_movement.y.abs() < ZERO_MOVEMENT_EPSILON);
    }

    #[test]
    fn test_sneak_edge_treats_entity_collision_as_support() {
        let state = player_state(DVec3::new(0.0, 1.0, 0.0))
            .with_on_ground(true)
            .with_backs_off_from_edge(true);
        let world = EntityBoxWorld {
            boxes: vec![WorldAabb::new(0.7, 0.4, -0.3, 1.3, 1.0, 0.3)],
        };
        let movement = DVec3::new(1.0, 0.0, 0.0);

        let result = move_entity(&state, movement, MoverType::Player, &world);

        assert_eq!(result.actual_movement, movement);
    }

    #[test]
    fn test_not_crouching_can_move_off_edge() {
        let state = player_state(DVec3::new(0.0, 1.0, 0.0)).with_on_ground(true);

        let world = BoxWorld {
            boxes: vec![WorldAabb::new(-2.0, 0.0, -2.0, 0.5, 1.0, 2.0)],
        };
        let movement = DVec3::new(1.0, 0.0, 0.0);

        let result = move_entity(&state, movement, MoverType::Player, &world);

        assert_eq!(result.actual_movement, movement);
    }

    #[test]
    fn test_entity_collision_clips_horizontal_movement() {
        let state = player_state(DVec3::new(0.0, 1.0, 0.0));
        let world = EntityBoxWorld {
            boxes: vec![WorldAabb::new(0.7, 1.0, -0.3, 1.7, 2.8, 0.3)],
        };

        let result = move_entity(
            &state,
            DVec3::new(1.0, 0.0, 0.0),
            MoverType::SelfMovement,
            &world,
        );

        assert!(
            result.actual_movement.x > 0.39 && result.actual_movement.x < 0.41,
            "entity collision should clip movement at the other entity's box: {:?}",
            result.actual_movement
        );
        assert!(result.horizontal_collision);
        assert!(result.x_collision);
    }

    #[test]
    fn test_world_border_collision_clips_horizontal_movement() {
        let state = player_state(DVec3::new(0.0, 1.0, 0.0));
        let world = BorderWorld {
            boxes: vec![WorldAabb::new(
                1.0,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::INFINITY,
                f64::INFINITY,
            )],
        };

        let result = move_entity(
            &state,
            DVec3::new(2.0, 0.0, 0.0),
            MoverType::SelfMovement,
            &world,
        );

        assert!(
            result.actual_movement.x > 0.69 && result.actual_movement.x < 0.71,
            "world border should clip movement at its outside shape: {:?}",
            result.actual_movement
        );
        assert!(result.horizontal_collision);
        assert!(result.x_collision);
    }

    #[test]
    fn test_step_up_uses_obstacle_candidate_height() {
        let state = player_state(DVec3::new(0.0, 1.0, 0.0)).with_on_ground(true);

        let world = BoxWorld {
            boxes: vec![
                WorldAabb::new(-10.0, 0.0, -10.0, 10.0, 1.0, 10.0),
                WorldAabb::new(0.5, 1.0, -1.0, 1.5, 1.5, 1.0),
            ],
        };

        let result = move_entity(
            &state,
            DVec3::new(1.0, 0.0, 0.0),
            MoverType::SelfMovement,
            &world,
        );

        assert!(
            result.actual_movement.x > 0.9,
            "step-up should preserve horizontal movement: {:?}",
            result.actual_movement
        );
        assert!((result.actual_movement.y - 0.5).abs() < ZERO_MOVEMENT_EPSILON);
    }

    #[test]
    fn test_step_up_rejects_obstacle_above_max_step() {
        let state = player_state(DVec3::new(0.0, 1.0, 0.0)).with_on_ground(true);

        let world = BoxWorld {
            boxes: vec![
                WorldAabb::new(-10.0, 0.0, -10.0, 10.0, 1.0, 10.0),
                WorldAabb::new(0.5, 1.0, -1.0, 1.5, 2.0, 1.0),
            ],
        };

        let result = move_entity(
            &state,
            DVec3::new(1.0, 0.0, 0.0),
            MoverType::SelfMovement,
            &world,
        );

        assert!(
            result.actual_movement.x < 0.3,
            "movement should stay clipped by the tall obstacle: {:?}",
            result.actual_movement
        );
        assert!(result.actual_movement.y.abs() < ZERO_MOVEMENT_EPSILON);
        assert!(result.horizontal_collision);
    }

    #[test]
    fn horizontal_collision_uses_vanilla_mth_equal_tolerance() {
        assert!(!horizontal_axis_collided(1.0, 1.0));
        assert!(!horizontal_axis_collided(1.0, 1.0 - 0.5e-5));
        assert!(horizontal_axis_collided(1.0, 1.0 - 1.1e-5));
        assert!(horizontal_axis_collided(1.0, 1.0 - 2.0e-5));
    }
}
