//! Common base functionality shared by all entities.
//!
//! `EntityBase` contains the core fields and methods that every entity needs.
//! Entities embed this struct and delegate common `Entity` trait methods to it.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Weak};

use steel_utils::locks::SyncMutex;
use steel_utils::math::Vector3;
use uuid::Uuid;

use crate::entity::{EntityLevelCallback, NullEntityCallback, RemovalReason};
use crate::world::World;

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
///     fn position(&self) -> Vector3<f64> { self.base.position() }
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
    world: Weak<World>,
    /// Current position in the world.
    position: SyncMutex<Vector3<f64>>,
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
    pub fn new(id: i32, position: Vector3<f64>, world: Weak<World>) -> Self {
        Self::with_uuid(id, Uuid::new_v4(), position, world)
    }

    /// Creates a new `EntityBase` with the specified UUID.
    ///
    /// Use this when loading entities from disk or when the UUID is known.
    #[must_use]
    pub fn with_uuid(id: i32, uuid: Uuid, position: Vector3<f64>, world: Weak<World>) -> Self {
        Self {
            id,
            uuid,
            world,
            position: SyncMutex::new(position),
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
    pub fn position(&self) -> Vector3<f64> {
        *self.position.lock()
    }

    /// Gets the world this entity is in.
    ///
    /// Returns `None` if the world has been dropped.
    #[inline]
    pub fn level(&self) -> Option<Arc<World>> {
        self.world.upgrade()
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

    /// Sets the level callback for lifecycle events.
    pub fn set_level_callback(&self, callback: Arc<dyn EntityLevelCallback>) {
        *self.level_callback.lock() = callback;
    }

    /// Sets the entity's position and notifies the callback.
    pub fn set_position(&self, pos: Vector3<f64>) {
        let old_pos = {
            let mut position = self.position.lock();
            let old = *position;
            *position = pos;
            old
        };
        self.level_callback.lock().on_move(old_pos, pos);
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
