//! Common base functionality shared by all entities.
//!
//! `EntityBase` contains the core fields and methods that every entity needs.
//! Entities embed this struct and delegate common `Entity` trait methods to it.

mod fire_freeze;
mod movement;
mod persistence;
mod relationships;

pub use fire_freeze::EntityFireFreezeState;
pub use movement::{
    EntityGroundContact, EntityMovement, EntityMovementEmission, EntityMovementFlags,
    EntityMovementProgress, EntityVerticalMovementStateUpdate,
};
pub use persistence::{EntityBaseLoad, EntityBaseSaveData};
pub use relationships::PendingWorldChangeToken;
use relationships::{EntityLifecycleState, EntityRelationshipState};

use std::{
    collections::VecDeque,
    mem,
    sync::{Arc, Weak},
};

use glam::DVec3;
use simdnbt::owned::NbtCompound;
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::EntityDimensions;
use steel_registry::vanilla_entities;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, WorldAabb};
use text_components::TextComponent;
use uuid::Uuid;

use crate::entity::fluid_contact::EntityFluidContact;
use crate::entity::{
    EntityLevelCallback, EntityMoveError, InsideBlockEffectType, NullEntityCallback, RemovalReason,
    SharedEntity,
};
use crate::physics::EntityPhysicsState;
use crate::portal::{PortalKind, PortalProcessResult, PortalProcessor};
use crate::world::World;

const BOARDING_COOLDOWN: i32 = 60;
const PISTON_MOVEMENT_LIMIT: f64 = 0.51;
const PISTON_ZERO_MOVEMENT_EPSILON: f64 = 1.0e-7;
const PISTON_APPLIED_MOVEMENT_EPSILON: f64 = 1.0e-5;
const STUCK_SPEED_MULTIPLIER_EPSILON: f64 = 1.0e-7;
const MOVEMENT_TRACE_LIMIT: usize = 100;
const MOVEMENT_TRACE_POSITION_EPSILON_SQ: f64 = 9.999_999_4e-11;
/// Default vanilla `Entity.getTicksRequiredToFreeze` value.
pub const DEFAULT_TICKS_REQUIRED_TO_FREEZE: i32 = 140;
/// Default vanilla `Entity.getMaxAirSupply` value.
pub const DEFAULT_MAX_AIR_SUPPLY: i32 = 300;
/// Vanilla scoreboard tag limit for a single entity.
pub const MAX_ENTITY_TAGS: usize = 1024;
const FIRE_IGNITE_TICKS: i32 = 8 * 20;
const LAVA_IGNITE_TICKS: i32 = 15 * 20;

fn require_finite_position(position: DVec3, field: &str) {
    assert!(
        position.is_finite(),
        "entity {field} must be finite: {position:?}"
    );
}

fn normalize_rotation(rotation: (f32, f32)) -> (f32, f32) {
    assert!(
        rotation.0.is_finite() && rotation.1.is_finite(),
        "entity rotation must be finite: {rotation:?}"
    );
    (rotation.0 % 360.0, rotation.1.clamp(-90.0, 90.0) % 360.0)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EntityPhysicsStateInput {
    pub(crate) max_up_step: f32,
    pub(crate) backs_off_from_edge: bool,
    pub(crate) descending: bool,
    pub(crate) can_walk_on_powder_snow: bool,
    pub(crate) is_falling_block: bool,
}

#[derive(Debug, Default)]
struct EntityMovementTrace {
    movement_this_tick: VecDeque<EntityMovement>,
    final_movements_this_tick: Vec<EntityMovement>,
}

impl EntityMovementTrace {
    fn record(&mut self, movement: EntityMovement) {
        if self.movement_this_tick.len() >= MOVEMENT_TRACE_LIMIT {
            let first = self.movement_this_tick.pop_front();
            let second = self.movement_this_tick.pop_front();
            match (first, second) {
                (Some(first), Some(second)) => self
                    .movement_this_tick
                    .push_front(EntityMovement::new(first.from(), second.to())),
                (Some(first), None) => self.movement_this_tick.push_front(first),
                (None, _) => {}
            }
        }

        self.movement_this_tick.push_back(movement);
    }

    fn remove_latest_recording(&mut self) {
        self.movement_this_tick.pop_back();
    }

    fn reset(&mut self) {
        self.movement_this_tick.clear();
        self.final_movements_this_tick.clear();
    }

    fn take_for_block_effects(
        &mut self,
        old_position: DVec3,
        position: DVec3,
    ) -> Vec<EntityMovement> {
        self.final_movements_this_tick.clear();
        self.final_movements_this_tick
            .extend(self.movement_this_tick.drain(..));

        if let Some(last_movement) = self.final_movements_this_tick.last().copied() {
            if (last_movement.to() - position).length_squared() > MOVEMENT_TRACE_POSITION_EPSILON_SQ
            {
                self.final_movements_this_tick
                    .push(EntityMovement::new(last_movement.to(), position));
            }
        } else {
            self.final_movements_this_tick
                .push(EntityMovement::new(old_position, position));
        }

        self.final_movements_this_tick.as_slice().to_vec()
    }

    fn last_for_block_effects(&self) -> Vec<EntityMovement> {
        self.final_movements_this_tick.as_slice().to_vec()
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

/// Sound parameters for vanilla amethyst step chimes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityAmethystStepSound {
    /// Chime volume.
    pub volume: f32,
    /// Chime pitch.
    pub pitch: f32,
}

/// Vanilla `Entity` movement state stored as one locked snapshot.
///
/// Position, velocity, rotation, and ground contact are commonly read together
/// by physics, saving, and future navigation code. Keeping them in one struct
/// makes those ownership boundaries explicit while still exposing focused
/// accessors through [`EntityBase`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityBaseState {
    tick_count: i32,
    first_tick: bool,
    position: DVec3,
    old_position: DVec3,
    last_known_position: Option<DVec3>,
    last_known_speed: DVec3,
    velocity: DVec3,
    rotation: (f32, f32),
    old_rotation: (f32, f32),
    pose: EntityPose,
    dimensions: EntityDimensions,
    bounding_box: WorldAabb,
    movement_flags: EntityMovementFlags,
    ground_contact: EntityGroundContact,
    movement_progress: EntityMovementProgress,
    fire_freeze: EntityFireFreezeState,
    in_block_state: Option<BlockStateId>,
    fluid_contact: EntityFluidContact,
    was_eye_in_water: bool,
    piston_movement: EntityPistonMovement,
    fall_distance: f64,
    stuck_speed_multiplier: DVec3,
    no_physics: bool,
    needs_velocity_sync: bool,
    hurt_marked: bool,
}

impl EntityBaseState {
    /// Creates base state for a freshly spawned entity.
    #[must_use]
    pub fn new(position: DVec3, dimensions: EntityDimensions) -> Self {
        require_finite_position(position, "position");
        Self {
            tick_count: 0,
            first_tick: true,
            position,
            old_position: position,
            last_known_position: None,
            last_known_speed: DVec3::ZERO,
            velocity: DVec3::ZERO,
            rotation: (0.0, 0.0),
            old_rotation: (0.0, 0.0),
            pose: EntityPose::Standing,
            dimensions,
            bounding_box: Self::make_bounding_box(position, dimensions),
            movement_flags: EntityMovementFlags::new(),
            ground_contact: EntityGroundContact::airborne(),
            movement_progress: EntityMovementProgress::new(),
            fire_freeze: EntityFireFreezeState::new(),
            in_block_state: None,
            fluid_contact: EntityFluidContact::default(),
            was_eye_in_water: false,
            piston_movement: EntityPistonMovement::new(),
            fall_distance: 0.0,
            stuck_speed_multiplier: DVec3::ZERO,
            no_physics: false,
            needs_velocity_sync: false,
            hurt_marked: false,
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
    pub fn with_velocity(mut self, velocity: DVec3) -> Self {
        if velocity.is_finite() {
            self.velocity = velocity;
        }
        self
    }

    /// Sets previous position on this state snapshot.
    #[must_use]
    pub fn with_old_position(mut self, old_position: DVec3) -> Self {
        require_finite_position(old_position, "old position");
        self.old_position = old_position;
        self
    }

    /// Sets rotation on this state snapshot.
    #[must_use]
    pub fn with_rotation(mut self, rotation: (f32, f32)) -> Self {
        let rotation = normalize_rotation(rotation);
        self.rotation = rotation;
        self.old_rotation = rotation;
        self
    }

    /// Sets accumulated fall distance on this state snapshot.
    #[must_use]
    pub const fn with_fall_distance(mut self, fall_distance: f64) -> Self {
        self.fall_distance = fall_distance;
        self
    }

    /// Sets base fire/freeze state on this construction snapshot.
    #[must_use]
    pub const fn with_fire_freeze_state(mut self, fire_freeze: EntityFireFreezeState) -> Self {
        self.fire_freeze = fire_freeze;
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
    /// Shared vanilla save data outside the movement snapshot.
    save_data: SyncMutex<EntityBaseSaveData>,
    /// Per-tick movement segments used by vanilla block-contact effects.
    movement_trace: SyncMutex<EntityMovementTrace>,
    /// Removal and tick bookkeeping.
    lifecycle: SyncMutex<EntityLifecycleState>,
    /// Passenger, vehicle, and boarding-cooldown state.
    relationships: SyncMutex<EntityRelationshipState>,
    /// Active vanilla portal timing state.
    portal_process: SyncMutex<Option<PortalProcessor>>,
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
    #[expect(
        clippy::large_types_passed_by_value,
        reason = "EntityBaseState is an owned construction snapshot built with with_* helpers"
    )]
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
    #[expect(
        clippy::large_types_passed_by_value,
        reason = "EntityBaseState is an owned construction snapshot built with with_* helpers"
    )]
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
            save_data: SyncMutex::new(EntityBaseSaveData::new()),
            movement_trace: SyncMutex::new(EntityMovementTrace::default()),
            lifecycle: SyncMutex::new(EntityLifecycleState::new()),
            relationships: SyncMutex::new(EntityRelationshipState::default()),
            portal_process: SyncMutex::new(None),
            level_callback: SyncMutex::new(Arc::new(NullEntityCallback)),
        }
    }

    /// Creates a base from persistent vanilla entity fields.
    #[must_use]
    pub fn from_load(load: EntityBaseLoad, dimensions: EntityDimensions) -> Self {
        let base = Self::with_uuid_and_state(
            load.id,
            load.uuid,
            EntityBaseState::new(load.position, dimensions)
                .with_velocity(load.velocity)
                .with_rotation(load.rotation)
                .with_fall_distance(load.fall_distance)
                .with_fire_freeze_state(load.fire_freeze)
                .with_on_ground(load.on_ground),
            load.world,
        );
        base.replace_save_data(load.save_data);
        base
    }

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

    /// Returns vanilla `lastKnownSpeed`, the displacement computed at base-tick start.
    #[inline]
    pub fn known_speed(&self) -> DVec3 {
        self.state.lock().last_known_speed
    }

    /// Returns vanilla `Entity.tickCount`.
    #[inline]
    pub fn tick_count(&self) -> i32 {
        self.state.lock().tick_count
    }

    /// Returns whether the entity has not completed its first tick.
    #[inline]
    pub fn is_first_tick(&self) -> bool {
        self.state.lock().first_tick
    }

    /// Sets whether the entity has not completed its first tick.
    #[inline]
    pub fn set_first_tick(&self, first_tick: bool) {
        self.state.lock().first_tick = first_tick;
    }

    /// Gets the entity's current bounding box.
    #[inline]
    pub fn bounding_box(&self) -> WorldAabb {
        self.state.lock().bounding_box
    }

    /// Returns the vanilla movement physics snapshot from the current base state.
    pub(crate) fn physics_state(&self, input: EntityPhysicsStateInput) -> EntityPhysicsState {
        let state = self.state.lock();
        EntityPhysicsState::new(state.position, state.bounding_box, input.max_up_step)
            .with_on_ground(state.movement_flags.on_ground())
            .with_backs_off_from_edge(input.backs_off_from_edge)
            .with_fall_distance(state.fall_distance)
            .with_descending(input.descending)
            .with_can_walk_on_powder_snow(input.can_walk_on_powder_snow)
            .with_falling_block(input.is_falling_block)
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

    /// Gets vanilla `yRotO`/`xRotO` as (yaw, pitch) in degrees.
    #[inline]
    pub fn old_rotation(&self) -> (f32, f32) {
        self.state.lock().old_rotation
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

    /// Returns the current vanilla ground-contact snapshot.
    #[inline]
    pub fn ground_contact(&self) -> EntityGroundContact {
        self.state.lock().ground_contact
    }

    /// Returns vanilla movement side-effect progress counters.
    #[inline]
    pub fn movement_progress(&self) -> EntityMovementProgress {
        self.state.lock().movement_progress
    }

    /// Returns the current vanilla fire/freeze state.
    #[inline]
    pub fn fire_freeze_state(&self) -> EntityFireFreezeState {
        self.state.lock().fire_freeze
    }

    /// Returns a snapshot of shared vanilla save data.
    pub fn save_data(&self) -> EntityBaseSaveData {
        self.save_data.lock().clone()
    }

    /// Replaces shared vanilla save data.
    pub fn replace_save_data(&self, save_data: EntityBaseSaveData) {
        *self.save_data.lock() = save_data;
    }

    /// Returns vanilla `Entity.getInBlockState`, cached until base tick or block-position change.
    pub fn in_block_state(&self, world: &World) -> BlockStateId {
        let mut state = self.state.lock();
        if let Some(in_block_state) = state.in_block_state {
            return in_block_state;
        }

        let position = state.position;
        let block_pos = BlockPos::containing(position.x, position.y, position.z);
        let in_block_state = world.get_block_state(block_pos);
        state.in_block_state = Some(in_block_state);
        in_block_state
    }

    /// Replaces the current vanilla fire/freeze state.
    pub fn set_fire_freeze_state(&self, fire_freeze: EntityFireFreezeState) {
        self.state.lock().fire_freeze = fire_freeze;
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

    /// Returns cached fluid contact from the last entity fluid refresh.
    pub fn fluid_contact(&self) -> EntityFluidContact {
        self.state.lock().fluid_contact
    }

    /// Returns vanilla `wasEyeInWater` from the previous fluid refresh.
    pub fn was_eye_in_water(&self) -> bool {
        self.state.lock().was_eye_in_water
    }

    /// Returns accumulated vanilla fall distance.
    #[inline]
    pub fn fall_distance(&self) -> f64 {
        self.state.lock().fall_distance
    }

    /// Returns true when movement bypasses collision physics.
    #[inline]
    pub fn no_physics(&self) -> bool {
        self.state.lock().no_physics
    }

    /// Returns the synchronized vanilla `Air` value.
    #[inline]
    pub fn air_supply(&self) -> i32 {
        self.save_data.lock().air_supply
    }

    /// Returns the vanilla portal cooldown in ticks.
    #[inline]
    pub fn portal_cooldown(&self) -> i32 {
        self.save_data.lock().portal_cooldown
    }

    /// Returns whether the entity is on vanilla portal cooldown.
    #[inline]
    pub fn is_on_portal_cooldown(&self) -> bool {
        self.portal_cooldown() > 0
    }

    /// Returns the active vanilla portal process, if the entity is charging a portal.
    #[inline]
    pub fn portal_process(&self) -> Option<PortalProcessor> {
        *self.portal_process.lock()
    }

    /// Returns the shared vanilla `NoGravity` flag.
    #[inline]
    pub fn no_gravity(&self) -> bool {
        self.save_data.lock().no_gravity
    }

    /// Returns the shared vanilla `Invulnerable` flag.
    #[inline]
    pub fn invulnerable(&self) -> bool {
        self.save_data.lock().invulnerable
    }

    /// Returns the optional vanilla custom name.
    #[inline]
    pub fn custom_name(&self) -> Option<TextComponent> {
        self.save_data.lock().custom_name.clone()
    }

    /// Returns the vanilla custom-name visibility flag.
    #[inline]
    pub fn custom_name_visible(&self) -> bool {
        self.save_data.lock().custom_name_visible
    }

    /// Returns the synchronized vanilla silent flag.
    #[inline]
    pub fn silent(&self) -> bool {
        self.save_data.lock().silent
    }

    /// Returns the server-owned vanilla glowing tag flag.
    #[inline]
    pub fn glowing(&self) -> bool {
        self.save_data.lock().glowing
    }

    /// Returns a sorted snapshot of vanilla scoreboard tags.
    pub fn tags(&self) -> Vec<String> {
        self.save_data.lock().tags.iter().cloned().collect()
    }

    /// Returns a snapshot of vanilla custom data.
    pub fn custom_data(&self) -> NbtCompound {
        self.save_data.lock().custom_data.clone()
    }

    /// Returns true when vanilla `ServerEntity` should consider a velocity sync.
    #[inline]
    pub fn needs_velocity_sync(&self) -> bool {
        self.state.lock().needs_velocity_sync
    }

    /// Returns true when vanilla hurt-marked velocity sync is pending.
    #[inline]
    pub fn hurt_marked(&self) -> bool {
        self.state.lock().hurt_marked
    }

    /// Gets the world this entity is in.
    ///
    /// Returns `None` if the world has been dropped.
    #[inline]
    pub fn level(&self) -> Option<Arc<World>> {
        self.world.lock().upgrade()
    }

    /// Gets the vehicle this entity is riding, if it is still loaded.
    pub fn vehicle(&self) -> Option<SharedEntity> {
        self.relationships.lock().vehicle()
    }

    /// Gets this entity's direct passengers, pruning stale weak references.
    pub fn passengers(&self) -> Vec<SharedEntity> {
        self.relationships.lock().passengers()
    }

    /// Gets this entity's first direct passenger, if present.
    pub fn first_passenger(&self) -> Option<SharedEntity> {
        self.relationships.lock().first_passenger()
    }

    /// Returns true when this entity has at least one direct passenger.
    pub fn is_vehicle(&self) -> bool {
        self.first_passenger().is_some()
    }

    /// Returns true when the entity ID is a direct passenger.
    pub fn has_passenger_id(&self, passenger_id: i32) -> bool {
        self.relationships.lock().has_passenger_id(passenger_id)
    }

    /// Returns the vanilla boarding cooldown in ticks.
    pub fn boarding_cooldown(&self) -> i32 {
        self.relationships.lock().boarding_cooldown
    }

    /// Removes a direct passenger by entity ID.
    pub(crate) fn remove_passenger_id(&self, passenger_id: i32) -> bool {
        self.relationships.lock().remove_passenger_id(passenger_id)
    }

    /// Stops riding the current vehicle, if any.
    pub fn stop_riding(&self) {
        self.stop_riding_relationship();
    }

    /// Restores a persisted passenger relationship without applying gameplay boarding rules.
    pub(crate) fn restore_passenger_relationship(vehicle: &SharedEntity, passenger: &SharedEntity) {
        passenger.base().stop_riding_relationship();
        Self::add_passenger_relationship(vehicle, passenger);
    }

    /// Starts a gameplay passenger relationship after vanilla boarding rules pass.
    pub(crate) fn start_riding_relationship(vehicle: &SharedEntity, passenger: &SharedEntity) {
        passenger.base().stop_riding_relationship();
        Self::add_passenger_relationship(vehicle, passenger);
    }

    fn add_passenger_relationship(vehicle: &SharedEntity, passenger: &SharedEntity) {
        if vehicle.base().has_passenger_id(passenger.id()) {
            return;
        }

        passenger.base().relationships.lock().vehicle = Some(Arc::downgrade(vehicle));
        let passenger_ref = Arc::downgrade(passenger);
        let mut vehicle_relationships = vehicle.base().relationships.lock();
        let first_passenger_is_player = vehicle_relationships
            .first_passenger()
            .is_some_and(|first| first.entity_type() == &vanilla_entities::PLAYER);
        if passenger.entity_type() == &vanilla_entities::PLAYER && !first_passenger_is_player {
            vehicle_relationships.passengers.insert(0, passenger_ref);
        } else {
            vehicle_relationships.passengers.push(passenger_ref);
        }
    }

    /// Sets the vanilla boarding cooldown in ticks.
    pub(crate) fn set_boarding_cooldown(&self, boarding_cooldown: i32) {
        self.relationships.lock().boarding_cooldown = boarding_cooldown;
    }

    /// Advances the base-tick movement and relationship state Steel currently implements.
    pub fn advance_base_tick_state(&self) {
        self.clear_in_block_state_for_base_tick();
        self.set_old_rotation_to_current();
        self.compute_known_speed();
        self.decrement_boarding_cooldown();
    }

    /// Clears vanilla `inBlockState` at the start of base tick.
    fn clear_in_block_state_for_base_tick(&self) {
        self.state.lock().in_block_state = None;
    }

    /// Computes vanilla `lastKnownSpeed` from the previous base-tick position.
    pub fn compute_known_speed(&self) {
        let mut state = self.state.lock();
        let previous_position = match state.last_known_position {
            Some(position) => position,
            None => state.position,
        };
        state.last_known_speed = state.position - previous_position;
        state.last_known_position = Some(state.position);
    }

    fn decrement_boarding_cooldown(&self) {
        let mut relationships = self.relationships.lock();
        if relationships.boarding_cooldown > 0 {
            relationships.boarding_cooldown -= 1;
        }
    }

    /// Advances vanilla portal cooldown by one server tick.
    pub fn process_portal_cooldown(&self) {
        let mut save_data = self.save_data.lock();
        if save_data.portal_cooldown > 0 {
            save_data.portal_cooldown -= 1;
        }
    }

    /// Resets state that vanilla gets from constructing a fresh player entity for death respawn.
    pub fn reset_for_player_respawn(&self, dimensions: EntityDimensions) {
        self.reset_for_player_respawn_inner(dimensions, None);
    }

    /// Resets death-respawn state while retaining the relocation that owns admission.
    pub(crate) fn reset_for_player_respawn_during_world_change(
        &self,
        dimensions: EntityDimensions,
        pending_token: PendingWorldChangeToken,
    ) {
        self.reset_for_player_respawn_inner(dimensions, Some(pending_token));
    }

    fn reset_for_player_respawn_inner(
        &self,
        dimensions: EntityDimensions,
        pending_world_change: Option<PendingWorldChangeToken>,
    ) {
        let bounding_box = {
            let mut state = self.state.lock();
            let position = state.position;
            state.old_position = position;
            state.last_known_position = None;
            state.last_known_speed = DVec3::ZERO;
            state.velocity = DVec3::ZERO;
            state.old_rotation = state.rotation;
            state.pose = EntityPose::Standing;
            state.dimensions = dimensions;
            state.bounding_box = EntityBaseState::make_bounding_box(position, dimensions);
            state.movement_flags = EntityMovementFlags::new();
            state.ground_contact = EntityGroundContact::airborne();
            state.movement_progress = EntityMovementProgress::new();
            state.fire_freeze = EntityFireFreezeState::new();
            state.in_block_state = None;
            state.fluid_contact = EntityFluidContact::default();
            state.was_eye_in_water = false;
            state.piston_movement = EntityPistonMovement::new();
            state.fall_distance = 0.0;
            state.stuck_speed_multiplier = DVec3::ZERO;
            state.no_physics = false;
            state.needs_velocity_sync = false;
            state.hurt_marked = false;
            state.bounding_box
        };
        self.notify_bounding_box_changed(bounding_box);

        self.movement_trace.lock().reset();
        *self.portal_process.lock() = None;
        self.lifecycle.lock().pending_world_change = pending_world_change;

        let mut save_data = self.save_data.lock();
        let tags = mem::take(&mut save_data.tags);
        *save_data = EntityBaseSaveData::new();
        save_data.tags = tags;
    }

    /// Updates the world reference used by this entity.
    pub(crate) fn set_world(&self, world: Weak<World>) {
        *self.world.lock() = world;
    }

    /// Marks this entity as waiting for a prepared world change.
    pub fn begin_pending_world_change(&self) -> Option<PendingWorldChangeToken> {
        let mut lifecycle = self.lifecycle.lock();
        if lifecycle.removal_reason.is_some() || lifecycle.pending_world_change.is_some() {
            return None;
        }
        let token = lifecycle.next_world_change_token();
        lifecycle.pending_world_change = Some(token);
        Some(token)
    }

    /// Marks a live or killed player as waiting for respawn preparation.
    ///
    /// Killed players remain eligible because their async spawn search may need
    /// to be retried after the death animation removes their live entity.
    pub(crate) fn begin_pending_player_respawn(&self) -> Option<PendingWorldChangeToken> {
        let mut lifecycle = self.lifecycle.lock();
        if !matches!(lifecycle.removal_reason, None | Some(RemovalReason::Killed))
            || lifecycle.pending_world_change.is_some()
        {
            return None;
        }
        let token = lifecycle.next_world_change_token();
        lifecycle.pending_world_change = Some(token);
        Some(token)
    }

    /// Clears a pending world change if it still matches the provided token.
    pub fn finish_pending_world_change(&self, token: PendingWorldChangeToken) -> bool {
        let mut lifecycle = self.lifecycle.lock();
        if lifecycle.pending_world_change != Some(token) {
            return false;
        }
        lifecycle.pending_world_change = None;
        true
    }

    /// Returns true while this entity is waiting for a prepared world change.
    #[inline]
    pub fn is_world_change_pending(&self) -> bool {
        self.lifecycle.lock().pending_world_change.is_some()
    }

    /// Returns true if the given world-change token is still pending.
    #[inline]
    pub fn is_world_change_token_pending(&self, token: PendingWorldChangeToken) -> bool {
        self.lifecycle.lock().pending_world_change == Some(token)
    }

    /// Returns true if the entity has been marked for removal.
    #[inline]
    pub fn is_removed(&self) -> bool {
        self.lifecycle.lock().removal_reason.is_some()
    }

    /// Returns the reason this entity was removed, if it has been removed.
    #[inline]
    pub fn removal_reason(&self) -> Option<RemovalReason> {
        self.lifecycle.lock().removal_reason
    }

    /// Marks the entity as removed with the given reason.
    ///
    /// Notifies the level callback on first removal.
    pub fn set_removed(&self, reason: RemovalReason) {
        let callback = {
            let mut lifecycle = self.lifecycle.lock();
            if lifecycle.removal_reason.is_some() {
                None
            } else {
                lifecycle.removal_reason = Some(reason);
                lifecycle.pending_world_change = None;
                Some(self.level_callback.lock().clone())
            }
        };

        if let Some(callback) = callback {
            self.detach_from_relationships(reason);
            callback.on_remove(reason);
            *self.level_callback.lock() = Arc::new(NullEntityCallback);
        }
    }

    fn detach_from_relationships(&self, reason: RemovalReason) {
        if reason.should_destroy() {
            self.stop_riding_relationship();
        }
        self.eject_passenger_relationships();
    }

    fn stop_riding_relationship(&self) {
        let vehicle = {
            let mut relationships = self.relationships.lock();
            let vehicle = relationships.vehicle();
            relationships.vehicle = None;
            vehicle
        };

        if let Some(vehicle) = vehicle {
            vehicle.base().remove_passenger_id(self.id);
            self.set_boarding_cooldown(BOARDING_COOLDOWN);
        }
    }

    fn eject_passenger_relationships(&self) {
        let passengers = {
            let mut relationships = self.relationships.lock();
            let passengers = relationships.passengers();
            relationships.passengers.clear();
            passengers
        };

        for passenger in passengers {
            if passenger.base().clear_vehicle_if(self.id) {
                passenger.base().set_boarding_cooldown(BOARDING_COOLDOWN);
            }
        }
    }

    fn clear_vehicle_if(&self, vehicle_id: i32) -> bool {
        {
            let mut relationships = self.relationships.lock();
            let Some(vehicle) = relationships.vehicle() else {
                return false;
            };
            if vehicle.id() != vehicle_id {
                return false;
            }
        }

        if let Err(error) = self.try_set_position(self.position()) {
            log::warn!(
                "Failed to refresh passenger {} manager position before clearing vehicle {vehicle_id}: {error}",
                self.id
            );
        }

        let mut relationships = self.relationships.lock();
        let Some(vehicle) = relationships.vehicle() else {
            return false;
        };
        if vehicle.id() != vehicle_id {
            return false;
        }
        relationships.vehicle = None;
        true
    }

    /// Clears the removed flag and returns whether the entity had been removed.
    ///
    /// Steel reuses the same `Player` instance across respawn while vanilla
    /// constructs a fresh `ServerPlayer`, so player respawn needs an explicit
    /// way to reset this base lifecycle flag.
    pub fn clear_removed(&self) -> bool {
        let mut lifecycle = self.lifecycle.lock();
        let was_removed = lifecycle.removal_reason.is_some();
        lifecycle.removal_reason = None;
        lifecycle.pending_world_change = None;
        was_removed
    }

    /// Sets the level callback for lifecycle events.
    pub fn set_level_callback(&self, callback: Arc<dyn EntityLevelCallback>) {
        *self.level_callback.lock() = callback;
    }

    /// Sets the entity's position through the active level callback.
    #[must_use = "movement commits can fail when world entity state rejects the update"]
    pub fn try_set_position(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        require_finite_position(pos, "position");
        let old_pos = self.state.lock().position;
        let callback = self.level_callback.lock().clone();
        callback.validate_move(old_pos, pos)?;
        self.set_position_local_unchecked(pos);
        if let Err(error) = callback.on_move_committed(old_pos, pos) {
            self.set_position_local_unchecked(old_pos);
            return Err(error);
        }
        Ok(())
    }

    /// Sets position without consulting world lifecycle callbacks.
    ///
    /// Use this for construction, loading, proto-staged entities, and tests.
    pub(crate) fn set_position_local(&self, pos: DVec3) {
        let callback = self.level_callback.lock().clone();
        assert!(
            callback.allows_local_position_update(),
            "entity {} local position update bypassed world entity manager",
            self.id
        );
        self.set_position_local_unchecked(pos);
    }

    fn set_position_local_unchecked(&self, pos: DVec3) {
        require_finite_position(pos, "position");
        {
            let mut state = self.state.lock();
            let old = state.position;
            state.position = pos;
            state.bounding_box = EntityBaseState::make_bounding_box(pos, state.dimensions);
            if BlockPos::containing(old.x, old.y, old.z)
                != BlockPos::containing(pos.x, pos.y, pos.z)
            {
                state.in_block_state = None;
            }
        }
    }

    /// Sets the vanilla movement-trace old position to the current position.
    pub fn set_old_position_to_current(&self) {
        let mut state = self.state.lock();
        state.old_position = state.position;
    }

    /// Sets the vanilla movement-trace old position explicitly.
    pub fn set_old_position(&self, old_position: DVec3) {
        require_finite_position(old_position, "old position");
        self.state.lock().old_position = old_position;
    }

    /// Sets vanilla `yRotO`/`xRotO` to the current rotation.
    pub fn set_old_rotation_to_current(&self) {
        let mut state = self.state.lock();
        state.old_rotation = state.rotation;
    }

    /// Sets vanilla `yRotO` to the current yaw without changing `xRotO`.
    pub fn set_old_yaw_to_current(&self) {
        let mut state = self.state.lock();
        state.old_rotation.0 = state.rotation.0;
    }

    /// Sets vanilla `yRotO`/`xRotO` explicitly.
    pub fn set_old_rotation(&self, old_rotation: (f32, f32)) {
        self.state.lock().old_rotation = normalize_rotation(old_rotation);
    }

    /// Records a movement segment for vanilla block-contact effects.
    pub fn record_movement_this_tick(&self, movement: EntityMovement) {
        self.movement_trace.lock().record(movement);
    }

    /// Removes the latest movement segment recorded this tick.
    pub fn remove_latest_movement_recording(&self) {
        self.movement_trace.lock().remove_latest_recording();
    }

    /// Clears movement segments recorded for the current tick.
    pub fn clear_movement_this_tick(&self) {
        self.movement_trace.lock().reset();
    }

    /// Takes and finalizes this tick's movement segments for block-contact effects.
    pub fn take_movements_for_block_effects(&self) -> Vec<EntityMovement> {
        let (old_position, position) = {
            let state = self.state.lock();
            (state.old_position, state.position)
        };

        self.movement_trace
            .lock()
            .take_for_block_effects(old_position, position)
    }

    /// Returns the last finalized movement segments for vanilla block-contact effects.
    pub fn last_movements_for_block_effects(&self) -> Vec<EntityMovement> {
        self.movement_trace.lock().last_for_block_effects()
    }

    /// Sets the entity's bounding box directly.
    ///
    /// Use this for vanilla entities whose box is not simply dimensions centered
    /// on the entity position.
    pub fn set_bounding_box(&self, bounding_box: WorldAabb) {
        self.state.lock().bounding_box = bounding_box;
        self.notify_bounding_box_changed(bounding_box);
    }

    /// Sets pose and dimensions, then rebuilds the default position-centered box.
    pub fn set_pose_and_dimensions(&self, pose: EntityPose, dimensions: EntityDimensions) {
        let bounding_box = {
            let mut state = self.state.lock();
            state.pose = pose;
            state.dimensions = dimensions;
            state.bounding_box = EntityBaseState::make_bounding_box(state.position, dimensions);
            state.bounding_box
        };
        self.notify_bounding_box_changed(bounding_box);
    }

    fn notify_bounding_box_changed(&self, bounding_box: WorldAabb) {
        let callback = Arc::clone(&self.level_callback.lock());
        callback.on_bounding_box_changed(bounding_box);
    }

    /// Sets the entity's velocity in blocks per tick.
    pub fn set_velocity(&self, velocity: DVec3) {
        if velocity.is_finite() {
            self.state.lock().velocity = velocity;
        }
    }

    /// Advances vanilla `Entity.tickCount` by one tick.
    #[inline]
    pub fn advance_tick_count(&self) {
        let mut state = self.state.lock();
        state.tick_count = state.tick_count.wrapping_add(1);
    }

    /// Records movement distance used by vanilla step, swim, and flap effects.
    pub fn record_movement_progress(
        &self,
        clipped_movement: DVec3,
        climbing: bool,
    ) -> EntityMovementProgress {
        let mut state = self.state.lock();
        state
            .movement_progress
            .add_movement(clipped_movement, climbing);
        state.movement_progress
    }

    /// Stores vanilla `nextStep` after a produced movement side effect.
    pub fn set_next_step(&self, next_step: f32) {
        self.state.lock().movement_progress.next_step = next_step;
    }

    /// Returns vanilla amethyst-step chime parameters when the cooldown allows it.
    pub fn amethyst_step_sound(&self, tick_count: i32) -> Option<EntityAmethystStepSound> {
        let intensity = {
            let mut state = self.state.lock();
            let progress = &mut state.movement_progress;
            if tick_count < progress.last_crystal_sound_play_tick + 20 {
                return None;
            }

            let tick_delta = tick_count - progress.last_crystal_sound_play_tick;
            progress.crystal_sound_intensity *= 0.997_f32.powi(tick_delta);
            progress.crystal_sound_intensity = (progress.crystal_sound_intensity + 0.07).min(1.0);
            progress.last_crystal_sound_play_tick = tick_count;
            progress.crystal_sound_intensity
        };

        let pitch = 0.5 + intensity * rand::random::<f32>() * 1.2;
        let volume = 0.1 + intensity * 1.2;
        Some(EntityAmethystStepSound { volume, pitch })
    }

    /// Sets the entity's rotation as (yaw, pitch) in degrees.
    pub fn set_rotation(&self, rotation: (f32, f32)) {
        self.state.lock().rotation = normalize_rotation(rotation);
    }

    /// Sets whether this entity bypasses collision physics.
    pub fn set_no_physics(&self, no_physics: bool) {
        self.state.lock().no_physics = no_physics;
    }

    /// Sets the synchronized vanilla `Air` value.
    pub fn set_air_supply(&self, air_supply: i32) {
        self.save_data.lock().air_supply = air_supply;
    }

    /// Sets the vanilla portal cooldown in ticks.
    pub fn set_portal_cooldown(&self, portal_cooldown: i32) {
        self.save_data.lock().portal_cooldown = portal_cooldown;
    }

    /// Marks this entity as inside a vanilla portal during the current tick.
    pub fn set_as_inside_portal(&self, portal: PortalKind, entry_position: BlockPos) {
        let mut portal_process = self.portal_process.lock();
        match portal_process.as_mut() {
            Some(process) if process.is_same_portal(portal) => {
                process.set_as_inside_portal(entry_position);
            }
            _ => {
                *portal_process = Some(PortalProcessor::new(portal, entry_position));
            }
        }
    }

    /// Advances the active vanilla portal process, if one exists.
    pub fn process_portal_teleportation(
        &self,
        allowed_to_teleport: bool,
        transition_time: i32,
    ) -> Option<PortalProcessResult> {
        self.portal_process.lock().as_mut().map(|process| {
            process.process_portal_teleportation(allowed_to_teleport, transition_time)
        })
    }

    /// Clears the active vanilla portal process.
    pub fn clear_portal_process(&self) {
        *self.portal_process.lock() = None;
    }

    /// Sets the shared vanilla `NoGravity` flag.
    pub fn set_no_gravity(&self, no_gravity: bool) {
        self.save_data.lock().no_gravity = no_gravity;
    }

    /// Sets the shared vanilla `Invulnerable` flag.
    pub fn set_invulnerable(&self, invulnerable: bool) {
        self.save_data.lock().invulnerable = invulnerable;
    }

    /// Sets the optional vanilla custom name.
    pub fn set_custom_name(&self, custom_name: Option<TextComponent>) {
        self.save_data.lock().custom_name = custom_name;
    }

    /// Sets the vanilla custom-name visibility flag.
    pub fn set_custom_name_visible(&self, visible: bool) {
        self.save_data.lock().custom_name_visible = visible;
    }

    /// Sets the synchronized vanilla silent flag.
    pub fn set_silent(&self, silent: bool) {
        self.save_data.lock().silent = silent;
    }

    /// Sets the server-owned vanilla glowing tag flag.
    pub fn set_glowing(&self, glowing: bool) {
        self.save_data.lock().glowing = glowing;
    }

    /// Adds a vanilla scoreboard tag.
    pub fn add_tag(&self, tag: String) -> bool {
        self.save_data.lock().add_tag(tag)
    }

    /// Removes a vanilla scoreboard tag.
    pub fn remove_tag(&self, tag: &str) -> bool {
        self.save_data.lock().tags.remove(tag)
    }

    /// Replaces vanilla custom data.
    pub fn set_custom_data(&self, custom_data: NbtCompound) {
        self.save_data.lock().custom_data = custom_data;
    }

    /// Marks velocity for vanilla `ServerEntity` synchronization.
    pub fn mark_velocity_sync(&self) {
        self.state.lock().needs_velocity_sync = true;
    }

    /// Clears the vanilla velocity sync marker after send processing.
    pub fn clear_velocity_sync(&self) {
        self.state.lock().needs_velocity_sync = false;
    }

    /// Marks this entity as hurt for vanilla self-inclusive motion sync.
    pub fn mark_hurt(&self) {
        self.state.lock().hurt_marked = true;
    }

    /// Clears the vanilla hurt-marked motion sync flag.
    pub fn clear_hurt_mark(&self) {
        self.state.lock().hurt_marked = false;
    }

    /// Sets accumulated vanilla fall distance.
    pub fn set_fall_distance(&self, fall_distance: f64) {
        self.state.lock().fall_distance = fall_distance;
    }

    /// Adds vertical movement to accumulated fall distance using vanilla precision.
    pub fn accumulate_fall_distance(&self, vertical_movement: f64) {
        self.state.lock().fall_distance -= f64::from(vertical_movement as f32);
    }

    /// Resets accumulated vanilla fall distance.
    pub fn reset_fall_distance(&self) {
        self.set_fall_distance(0.0);
    }

    /// Returns vanilla `remainingFireTicks`.
    pub fn remaining_fire_ticks(&self) -> i32 {
        self.state.lock().fire_freeze.remaining_fire_ticks()
    }

    /// Sets vanilla `remainingFireTicks`.
    pub fn set_remaining_fire_ticks(&self, remaining_fire_ticks: i32) {
        self.state.lock().fire_freeze.remaining_fire_ticks = remaining_fire_ticks;
    }

    /// Returns synchronized vanilla `TicksFrozen`.
    pub fn ticks_frozen(&self) -> i32 {
        self.state.lock().fire_freeze.ticks_frozen()
    }

    /// Sets synchronized vanilla `TicksFrozen`.
    pub fn set_ticks_frozen(&self, ticks_frozen: i32) {
        self.state.lock().fire_freeze.ticks_frozen = ticks_frozen;
    }

    /// Returns whether the entity touched powder snow during the current tick.
    pub fn is_in_powder_snow(&self) -> bool {
        self.state.lock().fire_freeze.is_in_powder_snow()
    }

    /// Returns whether the entity touched powder snow during the previous tick.
    pub fn was_in_powder_snow(&self) -> bool {
        self.state.lock().fire_freeze.was_in_powder_snow()
    }

    /// Sets vanilla `hasVisualFire`.
    pub fn set_visual_fire(&self, has_visual_fire: bool) {
        self.state.lock().fire_freeze.has_visual_fire = has_visual_fire;
    }

    /// Returns vanilla `hasVisualFire`.
    pub fn has_visual_fire(&self) -> bool {
        self.state.lock().fire_freeze.has_visual_fire()
    }

    /// Returns whether the entity is on fire on the server.
    pub fn is_on_fire(&self, fire_immune: bool) -> bool {
        !fire_immune && self.remaining_fire_ticks() > 0
    }

    /// Returns whether the entity is freezing.
    pub fn is_freezing(&self) -> bool {
        self.state.lock().fire_freeze.is_freezing()
    }

    /// Returns whether the entity has reached full-freeze duration.
    pub fn is_fully_frozen(&self, ticks_required_to_freeze: i32) -> bool {
        self.state
            .lock()
            .fire_freeze
            .is_fully_frozen(ticks_required_to_freeze)
    }

    /// Advances vanilla powder-snow contact at the start of base tick.
    pub fn advance_powder_snow_contact_for_base_tick(&self) {
        let mut state = self.state.lock();
        state.fire_freeze.was_in_powder_snow = state.fire_freeze.is_in_powder_snow;
        state.fire_freeze.is_in_powder_snow = false;
    }

    /// Advances vanilla server-side fire tick state.
    ///
    /// Returns true when the caller should apply one tick of on-fire damage.
    pub fn advance_fire_tick(&self, fire_immune: bool, in_lava: bool) -> bool {
        let mut state = self.state.lock();
        if state.fire_freeze.remaining_fire_ticks <= 0 {
            return false;
        }

        if fire_immune {
            state.fire_freeze.remaining_fire_ticks = state.fire_freeze.remaining_fire_ticks.min(0);
            return false;
        }

        let should_damage = state.fire_freeze.remaining_fire_ticks % 20 == 0 && !in_lava;
        state.fire_freeze.remaining_fire_ticks -= 1;
        should_damage
    }

    /// Clears accumulated freezing.
    pub fn clear_freeze(&self) {
        self.set_ticks_frozen(0);
    }

    /// Clears fire without resetting the vanilla fire immunity cooldown.
    pub fn clear_fire(&self) {
        let mut state = self.state.lock();
        state.fire_freeze.remaining_fire_ticks = state.fire_freeze.remaining_fire_ticks.min(0);
    }

    /// Ignites this entity for a vanilla tick duration.
    pub fn ignite_for_ticks(&self, number_of_ticks: i32, remaining_fire_ticks_cap: Option<i32>) {
        let mut state = self.state.lock();
        Self::ignite_for_ticks_in_state(
            &mut state.fire_freeze,
            number_of_ticks,
            remaining_fire_ticks_cap,
        );
    }

    /// Applies a vanilla inside-block effect to base fire/freeze state.
    pub fn apply_inside_block_effect(
        &self,
        effect_type: InsideBlockEffectType,
        can_freeze: bool,
        fire_immune: bool,
        fire_ignite_extra_ticks: i32,
        ticks_required_to_freeze: i32,
        remaining_fire_ticks_cap: Option<i32>,
    ) {
        let mut state = self.state.lock();
        match effect_type {
            InsideBlockEffectType::Freeze => {
                state.fire_freeze.is_in_powder_snow = true;
                if can_freeze {
                    state.fire_freeze.ticks_frozen =
                        ticks_required_to_freeze.min(state.fire_freeze.ticks_frozen + 1);
                }
            }
            InsideBlockEffectType::ClearFreeze => {
                state.fire_freeze.ticks_frozen = 0;
            }
            InsideBlockEffectType::FireIgnite => {
                Self::apply_fire_ignite(
                    &mut state.fire_freeze,
                    fire_immune,
                    fire_ignite_extra_ticks,
                    remaining_fire_ticks_cap,
                );
            }
            InsideBlockEffectType::LavaIgnite => {
                if !fire_immune {
                    Self::ignite_for_ticks_in_state(
                        &mut state.fire_freeze,
                        LAVA_IGNITE_TICKS,
                        remaining_fire_ticks_cap,
                    );
                }
            }
            InsideBlockEffectType::Extinguish => {
                state.fire_freeze.remaining_fire_ticks =
                    state.fire_freeze.remaining_fire_ticks.min(0);
            }
        }
    }

    fn apply_fire_ignite(
        fire_freeze: &mut EntityFireFreezeState,
        fire_immune: bool,
        fire_ignite_extra_ticks: i32,
        remaining_fire_ticks_cap: Option<i32>,
    ) {
        if fire_immune {
            return;
        }

        if fire_freeze.remaining_fire_ticks < 0 {
            Self::set_remaining_fire_ticks_in_state(
                fire_freeze,
                fire_freeze.remaining_fire_ticks + 1,
                remaining_fire_ticks_cap,
            );
        } else if fire_ignite_extra_ticks > 0 {
            Self::set_remaining_fire_ticks_in_state(
                fire_freeze,
                fire_freeze.remaining_fire_ticks + fire_ignite_extra_ticks,
                remaining_fire_ticks_cap,
            );
        }

        if fire_freeze.remaining_fire_ticks >= 0 {
            Self::ignite_for_ticks_in_state(
                fire_freeze,
                FIRE_IGNITE_TICKS,
                remaining_fire_ticks_cap,
            );
        }
    }

    fn ignite_for_ticks_in_state(
        fire_freeze: &mut EntityFireFreezeState,
        number_of_ticks: i32,
        remaining_fire_ticks_cap: Option<i32>,
    ) {
        if fire_freeze.remaining_fire_ticks < number_of_ticks {
            Self::set_remaining_fire_ticks_in_state(
                fire_freeze,
                number_of_ticks,
                remaining_fire_ticks_cap,
            );
        }
        fire_freeze.ticks_frozen = 0;
    }

    fn set_remaining_fire_ticks_in_state(
        fire_freeze: &mut EntityFireFreezeState,
        remaining_fire_ticks: i32,
        remaining_fire_ticks_cap: Option<i32>,
    ) {
        fire_freeze.remaining_fire_ticks =
            Self::cap_remaining_fire_ticks(remaining_fire_ticks, remaining_fire_ticks_cap);
    }

    fn cap_remaining_fire_ticks(
        remaining_fire_ticks: i32,
        remaining_fire_ticks_cap: Option<i32>,
    ) -> i32 {
        remaining_fire_ticks_cap.map_or(remaining_fire_ticks, |cap| remaining_fire_ticks.min(cap))
    }

    /// Applies vanilla base-tick fall-distance damping while touching lava.
    pub fn dampen_fall_distance_in_lava(&self) {
        let mut state = self.state.lock();
        if !state.first_tick && state.fluid_contact.lava_height() > 0.0 {
            state.fall_distance *= 0.5;
        }
    }

    /// Applies vanilla fluid-interaction fall-distance reset while touching water.
    pub fn reset_fall_distance_in_water(&self) {
        let mut state = self.state.lock();
        if state.fluid_contact.water_height() > 0.0 {
            state.fall_distance = 0.0;
        }
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

    /// Stores the current vanilla supporting-block snapshot.
    pub fn set_ground_contact(&self, ground_contact: EntityGroundContact) {
        self.state.lock().ground_contact = ground_contact;
    }

    /// Stores the current vanilla fluid contact snapshot.
    pub fn set_fluid_contact(&self, fluid_contact: EntityFluidContact) {
        self.state.lock().fluid_contact = fluid_contact;
    }

    /// Returns whether the entity is currently touching lava.
    #[inline]
    pub fn is_in_lava(&self) -> bool {
        let state = self.state.lock();
        !state.first_tick && state.fluid_contact.lava_height() > 0.0
    }

    /// Stores fluid contact for a vanilla base-tick refresh.
    ///
    /// Vanilla updates `wasEyeInWater` from the previous fluid interaction
    /// before scanning the current one.
    pub fn set_fluid_contact_for_base_tick(&self, fluid_contact: EntityFluidContact) {
        let mut state = self.state.lock();
        state.was_eye_in_water = state.fluid_contact.eye_in_water();
        state.fluid_contact = fluid_contact;
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
}

#[cfg(test)]
mod tests;
