//! Common base functionality shared by all entities.
//!
//! `EntityBase` contains the core fields and methods that every entity needs.
//! Entities embed this struct and delegate common `Entity` trait methods to it.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_utils::locks::SyncMutex;
use uuid::Uuid;

use crate::entity::{EntityLevelCallback, NullEntityCallback, RemovalReason};
use crate::world::World;

/// Vanilla `Entity` movement state stored as one locked snapshot.
///
/// Position, velocity, rotation, and ground contact are commonly read together
/// by physics, saving, and future navigation code. Keeping them in one struct
/// makes those ownership boundaries explicit while still exposing focused
/// accessors through [`EntityBase`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityBaseState {
    position: DVec3,
    velocity: DVec3,
    rotation: (f32, f32),
    on_ground: bool,
}

impl EntityBaseState {
    /// Creates base state for a freshly spawned entity.
    #[must_use]
    pub const fn new(position: DVec3) -> Self {
        Self {
            position,
            velocity: DVec3::ZERO,
            rotation: (0.0, 0.0),
            on_ground: false,
        }
    }

    /// Sets velocity on this state snapshot.
    #[must_use]
    pub const fn with_velocity(mut self, velocity: DVec3) -> Self {
        self.velocity = velocity;
        self
    }

    /// Sets rotation on this state snapshot.
    #[must_use]
    pub const fn with_rotation(mut self, rotation: (f32, f32)) -> Self {
        self.rotation = rotation;
        self
    }

    /// Sets the ground-contact flag on this state snapshot.
    #[must_use]
    pub const fn with_on_ground(mut self, on_ground: bool) -> Self {
        self.on_ground = on_ground;
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
    /// Whether this entity has been removed.
    removed: AtomicBool,
    /// Callback for entity lifecycle events.
    level_callback: SyncMutex<Arc<dyn EntityLevelCallback>>,
    /// The server tick count when this entity was last ticked.
    /// Used to prevent double-ticking when moving between chunks.
    last_world_tick: AtomicI32,
}

impl EntityBase {
    /// Creates a new `EntityBase` with a randomly generated UUID.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_state(id, EntityBaseState::new(position), world)
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
    pub fn with_uuid(id: i32, uuid: Uuid, position: DVec3, world: Weak<World>) -> Self {
        Self::with_uuid_and_state(id, uuid, EntityBaseState::new(position), world)
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
            removed: AtomicBool::new(false),
            level_callback: SyncMutex::new(Arc::new(NullEntityCallback)),
            last_world_tick: AtomicI32::new(-1),
        }
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
        self.state.lock().on_ground
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
        self.removed.load(Ordering::Relaxed)
    }

    /// Marks the entity as removed with the given reason.
    ///
    /// Notifies the level callback on first removal.
    pub fn set_removed(&self, reason: RemovalReason) {
        if !self.removed.swap(true, Ordering::AcqRel) {
            self.level_callback.lock().on_remove(reason);
        }
    }

    /// Clears the removed flag and returns whether the entity had been removed.
    ///
    /// Steel reuses the same `Player` instance across respawn while vanilla
    /// constructs a fresh `ServerPlayer`, so player respawn needs an explicit
    /// way to reset this base lifecycle flag.
    pub fn clear_removed(&self) -> bool {
        self.removed.swap(false, Ordering::AcqRel)
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
            old
        };
        self.level_callback.lock().on_move(old_pos, pos);
    }

    /// Sets the entity's velocity in blocks per tick.
    pub fn set_velocity(&self, velocity: DVec3) {
        self.state.lock().velocity = velocity;
    }

    /// Sets the entity's rotation as (yaw, pitch) in degrees.
    pub fn set_rotation(&self, rotation: (f32, f32)) {
        self.state.lock().rotation = rotation;
    }

    /// Sets whether this entity is touching the ground.
    pub fn set_on_ground(&self, on_ground: bool) {
        self.state.lock().on_ground = on_ground;
    }

    /// Checks if this entity was already ticked during the given server tick.
    #[inline]
    pub fn was_ticked_this_tick(&self, server_tick: i32) -> bool {
        self.last_world_tick.load(Ordering::Acquire) == server_tick
    }

    /// Marks this entity as ticked for the given server tick.
    #[inline]
    pub fn mark_ticked(&self, server_tick: i32) {
        self.last_world_tick.store(server_tick, Ordering::Release);
    }
}
