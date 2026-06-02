//! Common base functionality shared by all entities.
//!
//! `EntityBase` contains the core fields and methods that every entity needs.
//! Entities embed this struct and delegate common `Entity` trait methods to it.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::EntityDimensions;
use steel_utils::BlockPos;
use steel_utils::WorldAabb;
use steel_utils::locks::SyncMutex;
use uuid::Uuid;

use crate::entity::{EntityLevelCallback, NullEntityCallback, RemovalReason};
use crate::world::World;

const PISTON_MOVEMENT_LIMIT: f64 = 0.51;
const PISTON_ZERO_MOVEMENT_EPSILON: f64 = 1.0e-7;
const PISTON_APPLIED_MOVEMENT_EPSILON: f64 = 1.0e-5;
const STUCK_SPEED_MULTIPLIER_EPSILON: f64 = 1.0e-7;

/// Vanilla collision and ground-contact flags updated by `Entity.move`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityMovementFlags {
    on_ground: bool,
    horizontal_collision: bool,
    vertical_collision: bool,
    vertical_collision_below: bool,
}

impl EntityMovementFlags {
    /// Creates movement flags for an entity that has not moved yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            on_ground: false,
            horizontal_collision: false,
            vertical_collision: false,
            vertical_collision_below: false,
        }
    }

    /// Creates movement flags from a completed movement pass.
    #[must_use]
    pub fn after_move(
        on_ground: bool,
        horizontal_collision: bool,
        vertical_collision: bool,
        requested_delta: DVec3,
    ) -> Self {
        Self {
            on_ground,
            horizontal_collision,
            vertical_collision,
            vertical_collision_below: vertical_collision && requested_delta.y < 0.0,
        }
    }

    /// Returns true if the entity is touching the ground.
    #[inline]
    #[must_use]
    pub const fn on_ground(self) -> bool {
        self.on_ground
    }

    /// Returns true if the last movement was clipped horizontally.
    #[inline]
    #[must_use]
    pub const fn horizontal_collision(self) -> bool {
        self.horizontal_collision
    }

    /// Returns true if the last movement was clipped vertically.
    #[inline]
    #[must_use]
    pub const fn vertical_collision(self) -> bool {
        self.vertical_collision
    }

    /// Returns true if the last vertical collision was below the entity.
    #[inline]
    #[must_use]
    pub const fn vertical_collision_below(self) -> bool {
        self.vertical_collision_below
    }

    /// Returns the same flags with a new ground-contact value.
    #[must_use]
    pub const fn with_on_ground(mut self, on_ground: bool) -> Self {
        self.on_ground = on_ground;
        self
    }

    /// Returns the same flags with a new horizontal-collision value.
    #[must_use]
    pub const fn with_horizontal_collision(mut self, horizontal_collision: bool) -> Self {
        self.horizontal_collision = horizontal_collision;
        self
    }

    /// Returns the same ground state with collision flags cleared.
    #[must_use]
    pub const fn without_collisions(mut self) -> Self {
        self.horizontal_collision = false;
        self.vertical_collision = false;
        self.vertical_collision_below = false;
        self
    }
}

impl Default for EntityMovementFlags {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-tick piston movement accumulated by vanilla `Entity.limitPistonMovement`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct EntityPistonMovement {
    deltas: [f64; 3],
    game_time: i64,
}

impl EntityPistonMovement {
    const fn new() -> Self {
        Self {
            deltas: [0.0; 3],
            game_time: 0,
        }
    }

    fn limit_movement(&mut self, movement: DVec3, current_game_time: i64) -> DVec3 {
        if movement.length_squared() <= PISTON_ZERO_MOVEMENT_EPSILON {
            return movement;
        }

        if current_game_time != self.game_time {
            self.deltas = [0.0; 3];
            self.game_time = current_game_time;
        }

        if movement.x != 0.0 {
            return self.apply_axis_restriction(0, movement.x, DVec3::X);
        }
        if movement.y != 0.0 {
            return self.apply_axis_restriction(1, movement.y, DVec3::Y);
        }
        if movement.z != 0.0 {
            return self.apply_axis_restriction(2, movement.z, DVec3::Z);
        }

        DVec3::ZERO
    }

    fn apply_axis_restriction(&mut self, axis: usize, amount: f64, unit: DVec3) -> DVec3 {
        let limited =
            (amount + self.deltas[axis]).clamp(-PISTON_MOVEMENT_LIMIT, PISTON_MOVEMENT_LIMIT);
        let applied = limited - self.deltas[axis];
        self.deltas[axis] = limited;

        if applied.abs() <= PISTON_APPLIED_MOVEMENT_EPSILON {
            DVec3::ZERO
        } else {
            unit * applied
        }
    }
}

/// Vanilla ground-support state updated by `Entity.checkSupportingBlock`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EntityGroundContact {
    supporting_block: Option<BlockPos>,
    on_ground_no_blocks: bool,
}

impl EntityGroundContact {
    /// Creates airborne ground-contact state.
    #[must_use]
    pub const fn airborne() -> Self {
        Self {
            supporting_block: None,
            on_ground_no_blocks: false,
        }
    }

    /// Creates grounded contact state from the support search result.
    #[must_use]
    pub const fn on_ground(supporting_block: Option<BlockPos>) -> Self {
        Self {
            supporting_block,
            on_ground_no_blocks: supporting_block.is_none(),
        }
    }

    /// Returns the supporting block selected by vanilla support rules.
    #[must_use]
    pub const fn supporting_block(self) -> Option<BlockPos> {
        self.supporting_block
    }

    /// Returns true when the entity is grounded but no block support was found.
    #[must_use]
    pub const fn on_ground_no_blocks(self) -> bool {
        self.on_ground_no_blocks
    }
}

/// Vanilla `Entity` movement state stored as one locked snapshot.
///
/// Position, velocity, rotation, and ground contact are commonly read together
/// by physics, saving, and future navigation code. Keeping them in one struct
/// makes those ownership boundaries explicit while still exposing focused
/// accessors through [`EntityBase`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityBaseState {
    position: DVec3,
    old_position: DVec3,
    velocity: DVec3,
    rotation: (f32, f32),
    pose: EntityPose,
    dimensions: EntityDimensions,
    bounding_box: WorldAabb,
    movement_flags: EntityMovementFlags,
    ground_contact: EntityGroundContact,
    piston_movement: EntityPistonMovement,
    fall_distance: f32,
    stuck_speed_multiplier: DVec3,
    no_physics: bool,
}

impl EntityBaseState {
    /// Creates base state for a freshly spawned entity.
    #[must_use]
    pub fn new(position: DVec3, dimensions: EntityDimensions) -> Self {
        Self {
            position,
            old_position: position,
            velocity: DVec3::ZERO,
            rotation: (0.0, 0.0),
            pose: EntityPose::Standing,
            dimensions,
            bounding_box: Self::make_bounding_box(position, dimensions),
            movement_flags: EntityMovementFlags::new(),
            ground_contact: EntityGroundContact::airborne(),
            piston_movement: EntityPistonMovement::new(),
            fall_distance: 0.0,
            stuck_speed_multiplier: DVec3::ZERO,
            no_physics: false,
        }
    }

    /// Creates base state with an explicit bounding box.
    ///
    /// Hanging entities and other special cases do not use the default
    /// dimensions-centered box.
    #[must_use]
    pub fn new_with_bounding_box(
        position: DVec3,
        dimensions: EntityDimensions,
        bounding_box: WorldAabb,
    ) -> Self {
        Self {
            bounding_box,
            ..Self::new(position, dimensions)
        }
    }

    #[must_use]
    fn make_bounding_box(position: DVec3, dimensions: EntityDimensions) -> WorldAabb {
        WorldAabb::entity_box(
            position.x,
            position.y,
            position.z,
            f64::from(dimensions.half_width()),
            f64::from(dimensions.height),
        )
    }

    /// Sets velocity on this state snapshot.
    #[must_use]
    pub const fn with_velocity(mut self, velocity: DVec3) -> Self {
        self.velocity = velocity;
        self
    }

    /// Sets previous position on this state snapshot.
    #[must_use]
    pub const fn with_old_position(mut self, old_position: DVec3) -> Self {
        self.old_position = old_position;
        self
    }

    /// Sets rotation on this state snapshot.
    #[must_use]
    pub const fn with_rotation(mut self, rotation: (f32, f32)) -> Self {
        self.rotation = rotation;
        self
    }

    /// Sets accumulated fall distance on this state snapshot.
    #[must_use]
    pub const fn with_fall_distance(mut self, fall_distance: f32) -> Self {
        self.fall_distance = fall_distance;
        self
    }

    /// Sets the ground-contact flag on this state snapshot.
    #[must_use]
    pub const fn with_on_ground(mut self, on_ground: bool) -> Self {
        self.movement_flags = self.movement_flags.with_on_ground(on_ground);
        self.ground_contact = if on_ground {
            EntityGroundContact::on_ground(None)
        } else {
            EntityGroundContact::airborne()
        };
        self
    }

    /// Sets pose and dimensions on this state snapshot.
    #[must_use]
    pub fn with_pose_and_dimensions(
        mut self,
        pose: EntityPose,
        dimensions: EntityDimensions,
    ) -> Self {
        self.pose = pose;
        self.dimensions = dimensions;
        self.bounding_box = Self::make_bounding_box(self.position, dimensions);
        self
    }
}

/// Base fields restored from persistent entity data.
///
/// Vanilla loads these fields through `Entity.load` before type-specific
/// entity data. Keeping them bundled makes the load boundary explicit and
/// prevents constructor signatures from drifting as base state grows.
#[derive(Debug, Clone)]
pub struct EntityBaseLoad {
    /// Fresh runtime ID from `next_entity_id()`.
    pub id: i32,
    /// Restored entity position.
    pub position: DVec3,
    /// Persisted entity UUID.
    pub uuid: Uuid,
    /// Restored velocity.
    pub velocity: DVec3,
    /// Restored yaw and pitch.
    pub rotation: (f32, f32),
    /// Restored accumulated fall distance.
    pub fall_distance: f32,
    /// Restored ground-contact flag.
    pub on_ground: bool,
    /// World reference for the loaded entity.
    pub world: Weak<World>,
}

/// Non-physical lifecycle state shared by every entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EntityLifecycleState {
    removed: bool,
    last_world_tick: i32,
}

impl EntityLifecycleState {
    const fn new() -> Self {
        Self {
            removed: false,
            last_world_tick: -1,
        }
    }
}

/// Common fields and methods shared by all entities.
///
/// Entities embed this struct to avoid duplicating core identity, position,
/// and lifecycle management code. The `Entity` trait implementation can then
/// delegate to `EntityBase` methods for common functionality.
///
/// # Example
///
/// ```ignore
/// pub struct MyEntity {
///     base: EntityBase,
///     // Entity-specific fields...
/// }
///
/// impl Entity for MyEntity {
///     fn id(&self) -> i32 { self.base.id() }
///     fn uuid(&self) -> Uuid { self.base.uuid() }
///     fn position(&self) -> DVec3 { self.base.position() }
///     // ... delegate other common methods ...
///
///     // Entity-specific implementations:
///     fn entity_type(&self) -> EntityTypeRef { vanilla_entities::MY_ENTITY }
///     fn tick(&self) { /* custom tick logic */ }
/// }
/// ```
pub struct EntityBase {
    /// Unique network ID for this entity (session-local).
    id: i32,
    /// Persistent UUID for this entity.
    uuid: Uuid,
    /// The world this entity is in.
    world: SyncMutex<Weak<World>>,
    /// Current vanilla movement state.
    state: SyncMutex<EntityBaseState>,
    /// Removal and tick bookkeeping.
    lifecycle: SyncMutex<EntityLifecycleState>,
    /// Callback for entity lifecycle events.
    level_callback: SyncMutex<Arc<dyn EntityLevelCallback>>,
}

impl EntityBase {
    /// Creates a new `EntityBase` with a randomly generated UUID.
    #[must_use]
    pub fn new(id: i32, position: DVec3, dimensions: EntityDimensions, world: Weak<World>) -> Self {
        Self::new_with_state(id, EntityBaseState::new(position, dimensions), world)
    }

    /// Creates a new `EntityBase` with a randomly generated UUID and explicit state.
    #[must_use]
    pub fn new_with_state(id: i32, state: EntityBaseState, world: Weak<World>) -> Self {
        Self::with_uuid_and_state(id, Uuid::new_v4(), state, world)
    }

    /// Creates a new `EntityBase` with the specified UUID.
    ///
    /// Use this when loading entities from disk or when the UUID is known.
    #[must_use]
    pub fn with_uuid(
        id: i32,
        uuid: Uuid,
        position: DVec3,
        dimensions: EntityDimensions,
        world: Weak<World>,
    ) -> Self {
        Self::with_uuid_and_state(id, uuid, EntityBaseState::new(position, dimensions), world)
    }

    /// Creates a new `EntityBase` with the specified UUID and restored movement state.
    ///
    /// Use this when loading entities from disk so the vanilla base fields are
    /// reconstructed in one place.
    #[must_use]
    pub fn with_uuid_and_state(
        id: i32,
        uuid: Uuid,
        state: EntityBaseState,
        world: Weak<World>,
    ) -> Self {
        Self {
            id,
            uuid,
            world: SyncMutex::new(world),
            state: SyncMutex::new(state),
            lifecycle: SyncMutex::new(EntityLifecycleState::new()),
            level_callback: SyncMutex::new(Arc::new(NullEntityCallback)),
        }
    }

    /// Creates a base from persistent vanilla entity fields.
    #[must_use]
    pub fn from_load(load: EntityBaseLoad, dimensions: EntityDimensions) -> Self {
        Self::with_uuid_and_state(
            load.id,
            load.uuid,
            EntityBaseState::new(load.position, dimensions)
                .with_velocity(load.velocity)
                .with_rotation(load.rotation)
                .with_fall_distance(load.fall_distance)
                .with_on_ground(load.on_ground),
            load.world,
        )
    }

    // === Accessors for Entity trait delegation ===

    /// Gets the entity's unique network ID.
    #[inline]
    pub const fn id(&self) -> i32 {
        self.id
    }

    /// Gets the entity's UUID.
    #[inline]
    pub const fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Gets the entity's current position.
    #[inline]
    pub fn position(&self) -> DVec3 {
        self.state.lock().position
    }

    /// Gets the entity position used by vanilla movement traces.
    #[inline]
    pub fn old_position(&self) -> DVec3 {
        self.state.lock().old_position
    }

    /// Gets the entity's current bounding box.
    #[inline]
    pub fn bounding_box(&self) -> WorldAabb {
        self.state.lock().bounding_box
    }

    /// Gets the entity's current pose.
    #[inline]
    pub fn pose(&self) -> EntityPose {
        self.state.lock().pose
    }

    /// Gets the entity's current dimensions.
    #[inline]
    pub fn dimensions(&self) -> EntityDimensions {
        self.state.lock().dimensions
    }

    /// Gets the entity's current velocity in blocks per tick.
    #[inline]
    pub fn velocity(&self) -> DVec3 {
        self.state.lock().velocity
    }

    /// Gets the entity's rotation as (yaw, pitch) in degrees.
    #[inline]
    pub fn rotation(&self) -> (f32, f32) {
        self.state.lock().rotation
    }

    /// Returns true if the entity is touching the ground.
    #[inline]
    pub fn on_ground(&self) -> bool {
        self.state.lock().movement_flags.on_ground()
    }

    /// Returns the current vanilla movement flag snapshot.
    #[inline]
    pub fn movement_flags(&self) -> EntityMovementFlags {
        self.state.lock().movement_flags
    }

    /// Returns true if the last movement was clipped horizontally.
    #[inline]
    pub fn horizontal_collision(&self) -> bool {
        self.state.lock().movement_flags.horizontal_collision()
    }

    /// Returns true if the last movement was clipped vertically.
    #[inline]
    pub fn vertical_collision(&self) -> bool {
        self.state.lock().movement_flags.vertical_collision()
    }

    /// Returns true if the last vertical collision was below the entity.
    #[inline]
    pub fn vertical_collision_below(&self) -> bool {
        self.state.lock().movement_flags.vertical_collision_below()
    }

    /// Returns the block currently supporting this entity, if known.
    pub fn supporting_block(&self) -> Option<BlockPos> {
        self.state.lock().ground_contact.supporting_block()
    }

    /// Returns true when the entity is grounded but no supporting block was found.
    pub fn on_ground_no_blocks(&self) -> bool {
        self.state.lock().ground_contact.on_ground_no_blocks()
    }

    /// Returns accumulated vanilla fall distance.
    #[inline]
    pub fn fall_distance(&self) -> f32 {
        self.state.lock().fall_distance
    }

    /// Returns true when movement bypasses collision physics.
    #[inline]
    pub fn no_physics(&self) -> bool {
        self.state.lock().no_physics
    }

    /// Gets the world this entity is in.
    ///
    /// Returns `None` if the world has been dropped.
    #[inline]
    pub fn level(&self) -> Option<Arc<World>> {
        self.world.lock().upgrade()
    }

    /// Updates the world reference used by this entity.
    pub fn set_world(&self, world: Weak<World>) {
        *self.world.lock() = world;
    }

    /// Returns true if the entity has been marked for removal.
    #[inline]
    pub fn is_removed(&self) -> bool {
        self.lifecycle.lock().removed
    }

    /// Marks the entity as removed with the given reason.
    ///
    /// Notifies the level callback on first removal.
    pub fn set_removed(&self, reason: RemovalReason) {
        let should_notify = {
            let mut lifecycle = self.lifecycle.lock();
            if lifecycle.removed {
                false
            } else {
                lifecycle.removed = true;
                true
            }
        };

        if should_notify {
            self.level_callback.lock().on_remove(reason);
        }
    }

    /// Clears the removed flag and returns whether the entity had been removed.
    ///
    /// Steel reuses the same `Player` instance across respawn while vanilla
    /// constructs a fresh `ServerPlayer`, so player respawn needs an explicit
    /// way to reset this base lifecycle flag.
    pub fn clear_removed(&self) -> bool {
        let mut lifecycle = self.lifecycle.lock();
        let was_removed = lifecycle.removed;
        lifecycle.removed = false;
        was_removed
    }

    /// Sets the level callback for lifecycle events.
    pub fn set_level_callback(&self, callback: Arc<dyn EntityLevelCallback>) {
        *self.level_callback.lock() = callback;
    }

    /// Sets the entity's position and notifies the callback.
    pub fn set_position(&self, pos: DVec3) {
        let old_pos = {
            let mut state = self.state.lock();
            let old = state.position;
            state.position = pos;
            state.bounding_box = EntityBaseState::make_bounding_box(pos, state.dimensions);
            old
        };
        self.level_callback.lock().on_move(old_pos, pos);
    }

    /// Sets the vanilla movement-trace old position to the current position.
    pub fn set_old_position_to_current(&self) {
        let mut state = self.state.lock();
        state.old_position = state.position;
    }

    /// Sets the vanilla movement-trace old position explicitly.
    pub fn set_old_position(&self, old_position: DVec3) {
        self.state.lock().old_position = old_position;
    }

    /// Sets the entity's bounding box directly.
    ///
    /// Use this for vanilla entities whose box is not simply dimensions centered
    /// on the entity position.
    pub fn set_bounding_box(&self, bounding_box: WorldAabb) {
        self.state.lock().bounding_box = bounding_box;
    }

    /// Sets pose and dimensions, then rebuilds the default position-centered box.
    pub fn set_pose_and_dimensions(&self, pose: EntityPose, dimensions: EntityDimensions) {
        let mut state = self.state.lock();
        state.pose = pose;
        state.dimensions = dimensions;
        state.bounding_box = EntityBaseState::make_bounding_box(state.position, dimensions);
    }

    /// Sets the entity's velocity in blocks per tick.
    pub fn set_velocity(&self, velocity: DVec3) {
        self.state.lock().velocity = velocity;
    }

    /// Sets the entity's rotation as (yaw, pitch) in degrees.
    pub fn set_rotation(&self, rotation: (f32, f32)) {
        self.state.lock().rotation = rotation;
    }

    /// Sets whether this entity bypasses collision physics.
    pub fn set_no_physics(&self, no_physics: bool) {
        self.state.lock().no_physics = no_physics;
    }

    /// Sets accumulated vanilla fall distance.
    pub fn set_fall_distance(&self, fall_distance: f32) {
        self.state.lock().fall_distance = fall_distance;
    }

    /// Resets accumulated vanilla fall distance.
    pub fn reset_fall_distance(&self) {
        self.set_fall_distance(0.0);
    }

    /// Sets whether this entity is touching the ground.
    pub fn set_on_ground(&self, on_ground: bool) {
        let mut state = self.state.lock();
        state.movement_flags = state.movement_flags.with_on_ground(on_ground);
        if !on_ground {
            state.ground_contact = EntityGroundContact::airborne();
        }
    }

    /// Sets all vanilla movement flags after `Entity.move`.
    pub fn set_movement_flags(
        &self,
        movement_flags: EntityMovementFlags,
        ground_contact: EntityGroundContact,
    ) {
        let mut state = self.state.lock();
        state.movement_flags = movement_flags;
        state.ground_contact = ground_contact;
    }

    /// Sets ground and horizontal collision flags from an accepted client move.
    pub fn set_on_ground_with_movement(
        &self,
        on_ground: bool,
        horizontal_collision: bool,
        ground_contact: EntityGroundContact,
    ) {
        let mut state = self.state.lock();
        state.movement_flags = state
            .movement_flags
            .with_on_ground(on_ground)
            .with_horizontal_collision(horizontal_collision);
        state.ground_contact = ground_contact;
    }

    /// Clears collision flags after a no-physics move.
    pub fn clear_collision_flags(&self) {
        let mut state = self.state.lock();
        state.movement_flags = state.movement_flags.without_collisions();
    }

    /// Applies vanilla per-tick piston movement accumulation.
    pub fn limit_piston_movement(&self, movement: DVec3, current_game_time: i64) -> DVec3 {
        self.state
            .lock()
            .piston_movement
            .limit_movement(movement, current_game_time)
    }

    /// Sets the speed multiplier used for the next stuck-in-block movement pass.
    pub fn make_stuck_in_block(&self, speed_multiplier: DVec3) {
        let mut state = self.state.lock();
        state.fall_distance = 0.0;
        state.stuck_speed_multiplier = speed_multiplier;
    }

    /// Applies and clears vanilla stuck-in-block speed state.
    #[must_use]
    pub fn consume_stuck_speed_multiplier(&self, movement: DVec3, apply_multiplier: bool) -> DVec3 {
        let mut state = self.state.lock();
        if state.stuck_speed_multiplier.length_squared() <= STUCK_SPEED_MULTIPLIER_EPSILON {
            return movement;
        }

        let stuck_speed_multiplier = state.stuck_speed_multiplier;
        state.stuck_speed_multiplier = DVec3::ZERO;
        state.velocity = DVec3::ZERO;

        if apply_multiplier {
            movement * stuck_speed_multiplier
        } else {
            movement
        }
    }

    /// Checks if this entity was already ticked during the given server tick.
    #[inline]
    pub fn was_ticked_this_tick(&self, server_tick: i32) -> bool {
        self.lifecycle.lock().last_world_tick == server_tick
    }

    /// Marks this entity as ticked for the given server tick.
    #[inline]
    pub fn mark_ticked(&self, server_tick: i32) {
        self.lifecycle.lock().last_world_tick = server_tick;
    }
}

#[cfg(test)]
mod tests {
    use super::{EntityBase, EntityMovementFlags, EntityPistonMovement};
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::entity_type::EntityDimensions;
    use steel_utils::locks::SyncMutex;

    use crate::entity::{EntityLevelCallback, RemovalReason};
    use crate::world::World;

    fn assert_vec3_close(left: DVec3, right: DVec3) {
        let diff = left - right;
        assert!(
            diff.length_squared() < 1.0e-24,
            "expected {left:?} to equal {right:?}"
        );
    }

    fn assert_f32_close(left: f32, right: f32) {
        assert!(
            (left - right).abs() < 1.0e-6,
            "expected {left:?} to equal {right:?}"
        );
    }

    #[derive(Default)]
    struct CountingCallback {
        removals: SyncMutex<Vec<RemovalReason>>,
    }

    impl EntityLevelCallback for CountingCallback {
        fn on_move(&self, _old_pos: DVec3, _new_pos: DVec3) {}

        fn on_remove(&self, reason: RemovalReason) {
            self.removals.lock().push(reason);
        }
    }

    #[test]
    fn piston_movement_is_limited_per_axis_per_tick() {
        let mut piston_movement = EntityPistonMovement::new();

        assert_vec3_close(
            piston_movement.limit_movement(DVec3::new(0.4, 0.0, 0.0), 10),
            DVec3::new(0.4, 0.0, 0.0),
        );
        assert_vec3_close(
            piston_movement.limit_movement(DVec3::new(0.4, 0.0, 0.0), 10),
            DVec3::new(0.11, 0.0, 0.0),
        );
        assert_vec3_close(
            piston_movement.limit_movement(DVec3::new(0.4, 0.0, 0.0), 10),
            DVec3::ZERO,
        );
    }

    #[test]
    fn piston_movement_resets_each_game_tick() {
        let mut piston_movement = EntityPistonMovement::new();

        assert_vec3_close(
            piston_movement.limit_movement(DVec3::new(0.51, 0.0, 0.0), 10),
            DVec3::new(0.51, 0.0, 0.0),
        );
        assert_vec3_close(
            piston_movement.limit_movement(DVec3::new(0.51, 0.0, 0.0), 11),
            DVec3::new(0.51, 0.0, 0.0),
        );
    }

    #[test]
    fn piston_movement_uses_first_non_zero_axis() {
        let mut piston_movement = EntityPistonMovement::new();

        assert_vec3_close(
            piston_movement.limit_movement(DVec3::new(0.2, 0.2, 0.2), 10),
            DVec3::new(0.2, 0.0, 0.0),
        );
    }

    #[test]
    fn piston_movement_keeps_sub_threshold_movement() {
        let mut piston_movement = EntityPistonMovement::new();
        let movement = DVec3::new(0.0, 0.0, 1.0e-4);

        assert_vec3_close(piston_movement.limit_movement(movement, 10), movement);
    }

    #[test]
    fn collision_flags_clear_without_changing_ground_state() {
        let flags = EntityMovementFlags::after_move(true, true, true, DVec3::new(0.0, -1.0, 0.0))
            .without_collisions();

        assert!(flags.on_ground());
        assert!(!flags.horizontal_collision());
        assert!(!flags.vertical_collision());
        assert!(!flags.vertical_collision_below());
    }

    #[test]
    fn lifecycle_state_tracks_removal_and_tick_guard() {
        let base = EntityBase::new(
            1,
            DVec3::ZERO,
            EntityDimensions::new(0.25, 0.25, 0.125),
            Weak::<World>::new(),
        );
        let callback = Arc::new(CountingCallback::default());
        base.set_level_callback(callback.clone());

        assert!(!base.is_removed());
        assert!(!base.was_ticked_this_tick(12));

        base.mark_ticked(12);
        assert!(base.was_ticked_this_tick(12));
        assert!(!base.was_ticked_this_tick(13));

        base.set_removed(RemovalReason::Discarded);
        base.set_removed(RemovalReason::Killed);
        assert!(base.is_removed());
        assert_eq!(*callback.removals.lock(), vec![RemovalReason::Discarded]);
        assert!(base.clear_removed());
        assert!(!base.clear_removed());
        assert!(!base.is_removed());
    }

    #[test]
    fn no_physics_is_stored_on_base_state() {
        let base = EntityBase::new(
            1,
            DVec3::ZERO,
            EntityDimensions::new(0.25, 0.25, 0.125),
            Weak::<World>::new(),
        );

        assert!(!base.no_physics());
        base.set_no_physics(true);
        assert!(base.no_physics());
    }

    #[test]
    fn old_position_is_explicit_movement_trace_state() {
        let base = EntityBase::new(
            1,
            DVec3::new(1.0, 2.0, 3.0),
            EntityDimensions::new(0.25, 0.25, 0.125),
            Weak::<World>::new(),
        );

        assert_vec3_close(base.old_position(), DVec3::new(1.0, 2.0, 3.0));
        base.set_position(DVec3::new(4.0, 5.0, 6.0));
        assert_vec3_close(base.position(), DVec3::new(4.0, 5.0, 6.0));
        assert_vec3_close(base.old_position(), DVec3::new(1.0, 2.0, 3.0));

        base.set_old_position_to_current();
        assert_vec3_close(base.old_position(), DVec3::new(4.0, 5.0, 6.0));
        base.set_old_position(DVec3::new(7.0, 8.0, 9.0));
        assert_vec3_close(base.old_position(), DVec3::new(7.0, 8.0, 9.0));
    }

    #[test]
    fn fall_distance_is_stored_on_base_state() {
        let base = EntityBase::new(
            1,
            DVec3::ZERO,
            EntityDimensions::new(0.25, 0.25, 0.125),
            Weak::<World>::new(),
        );

        base.set_fall_distance(4.5);
        assert_f32_close(base.fall_distance(), 4.5);
        base.reset_fall_distance();
        assert_f32_close(base.fall_distance(), 0.0);
    }

    #[test]
    fn stuck_speed_multiplier_resets_fall_distance_and_applies_once() {
        let base = EntityBase::new(
            1,
            DVec3::ZERO,
            EntityDimensions::new(0.25, 0.25, 0.125),
            Weak::<World>::new(),
        );
        base.set_velocity(DVec3::new(0.4, -0.2, 0.3));
        base.set_fall_distance(3.0);
        base.make_stuck_in_block(DVec3::new(0.8, 0.75, 0.8));

        assert_f32_close(base.fall_distance(), 0.0);
        assert_vec3_close(
            base.consume_stuck_speed_multiplier(DVec3::new(1.0, -1.0, 0.5), true),
            DVec3::new(0.8, -0.75, 0.4),
        );
        assert_vec3_close(base.velocity(), DVec3::ZERO);
        assert_vec3_close(
            base.consume_stuck_speed_multiplier(DVec3::new(1.0, -1.0, 0.5), true),
            DVec3::new(1.0, -1.0, 0.5),
        );
    }

    #[test]
    fn stuck_speed_multiplier_can_be_consumed_without_applying_for_pistons() {
        let base = EntityBase::new(
            1,
            DVec3::ZERO,
            EntityDimensions::new(0.25, 0.25, 0.125),
            Weak::<World>::new(),
        );
        base.set_velocity(DVec3::new(0.4, -0.2, 0.3));
        base.make_stuck_in_block(DVec3::new(0.8, 0.75, 0.8));

        let movement = DVec3::new(1.0, -1.0, 0.5);
        assert_vec3_close(
            base.consume_stuck_speed_multiplier(movement, false),
            movement,
        );
        assert_vec3_close(base.velocity(), DVec3::ZERO);
    }
}
