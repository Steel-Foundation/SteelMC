//! This module contains entity-related traits and types.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Weak};

use simdnbt::borrow::BaseNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_registry::blocks::shapes::AABBd;
use steel_registry::entity_data::DataValue;
use steel_registry::entity_types::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_utils::math::Vector3;
use uuid::Uuid;

use crate::physics::{
    EntityPhysicsState, MoveResult, MoverType, WorldCollisionProvider, move_entity,
};
use crate::world::World;
use crate::{inventory::equipment::EquipmentSlot, player::Player};

use entities::ItemEntity;

/// Global counter for allocating unique entity IDs.
///
/// Mirrors vanilla's `Entity.ENTITY_COUNTER`. Each new entity increments this
/// counter to get a unique network ID. Starts at 1 (0 is reserved).
static ENTITY_COUNTER: AtomicI32 = AtomicI32::new(1);

/// Allocates a new unique entity ID.
///
/// This is the primary way to get entity IDs for spawning entities.
/// Thread-safe and lock-free.
#[must_use]
pub fn next_entity_id() -> i32 {
    ENTITY_COUNTER.fetch_add(1, Ordering::Relaxed)
}

mod base;
mod cache;
mod callback;
pub mod entities;
mod registry;
mod storage;
mod tracker;

pub use base::EntityBase;
pub use cache::EntityCache;
pub use callback::{
    EntityChunkCallback, EntityLevelCallback, NullEntityCallback, PlayerEntityCallback,
    RemovalReason,
};
pub use registry::{ENTITIES, EntityRegistry, init_entities};
pub use storage::EntityStorage;
pub use tracker::EntityTracker;

/// Type alias for a shared entity reference.
pub type SharedEntity = Arc<dyn Entity>;

/// Type alias for a weak entity reference.
pub type WeakEntity = Weak<dyn Entity>;

/// A trait for entities.
///
/// This trait provides the core functionality for entities.
/// It's based on Minecraft's `Entity` class.
///
/// # Using `EntityBase`
///
/// Entities that embed [`EntityBase`] can implement `base()` to get default
/// implementations for common methods like `id()`, `uuid()`, `position()`, etc.
///
/// ```ignore
/// impl Entity for MyEntity {
///     fn base(&self) -> Option<&EntityBase> { Some(&self.base) }
///     fn entity_type(&self) -> EntityTypeRef { vanilla_entities::MY_ENTITY }
///     fn bounding_box(&self) -> AABBd { /* ... */ }
///     // All other common methods use defaults from EntityBase!
/// }
/// ```
pub trait Entity: Send + Sync {
    /// Returns a reference to the entity's base fields, if available.
    ///
    /// Implement this to get default implementations for common methods.
    /// Returns `None` by default (for entities like Player that don't use `EntityBase`).
    fn base(&self) -> Option<&EntityBase> {
        None
    }

    /// Gets the entity type containing tracking range, dimensions, etc.
    fn entity_type(&self) -> EntityTypeRef;

    /// Gets the entity's unique network ID (session-local).
    fn id(&self) -> i32 {
        self.base().map_or(0, EntityBase::id)
    }

    /// Gets the UUID of the entity (persistent identifier).
    fn uuid(&self) -> Uuid {
        self.base().map_or(Uuid::nil(), EntityBase::uuid)
    }

    /// Gets the entity's current position.
    fn position(&self) -> Vector3<f64> {
        self.base()
            .map_or(Vector3::new(0.0, 0.0, 0.0), EntityBase::position)
    }

    /// Gets the entity's bounding box for collision queries.
    fn bounding_box(&self) -> AABBd;

    /// Called every game tick when the entity is in a ticked chunk.
    ///
    /// Use `self.level()` to access the world for physics, block queries, etc.
    /// The caller (`EntityStorage`) handles base tick logic like dirty data sync.
    fn tick(&self) {}

    /// Sends position/velocity changes to tracking players.
    ///
    /// Called every tick by `EntityStorage` after `tick()`, mirrors vanilla's
    /// `ServerEntity.sendChanges()`. Handles position sync based on `updateInterval`,
    /// velocity sync when `needsSync` is set, and on-ground state changes.
    ///
    /// Default implementation does nothing. Override for entities that need
    /// position/velocity synchronization (items, projectiles, etc.).
    fn send_changes(&self, _tick_count: i32) {}

    /// Gets the world this entity is in.
    ///
    /// Returns `None` if the entity is not in a world or the world was dropped.
    /// Mirrors vanilla's `Entity.level()`.
    fn level(&self) -> Option<Arc<World>> {
        self.base().and_then(EntityBase::level)
    }

    /// Packs dirty entity data for network synchronization.
    ///
    /// Returns `Some(values)` if there are dirty values to sync, `None` otherwise.
    /// Clears the dirty flags after packing.
    fn pack_dirty_entity_data(&self) -> Option<Vec<DataValue>> {
        None
    }

    /// Packs all non-default entity data for initial spawn.
    ///
    /// Used when sending entity data to a player who just started tracking this entity.
    fn pack_all_entity_data(&self) -> Vec<DataValue> {
        Vec::new()
    }

    /// Returns true if the entity has been marked for removal.
    fn is_removed(&self) -> bool {
        self.base().is_some_and(EntityBase::is_removed)
    }

    /// Marks the entity as removed with the given reason.
    fn set_removed(&self, reason: RemovalReason) {
        if let Some(base) = self.base() {
            base.set_removed(reason);
        }
    }

    /// Sets the level callback for lifecycle events (movement, removal).
    fn set_level_callback(&self, callback: Arc<dyn EntityLevelCallback>) {
        if let Some(base) = self.base() {
            base.set_level_callback(callback);
        }
    }

    /// Gets the entity as a Player if it is one.
    fn as_player(self: Arc<Self>) -> Option<Arc<Player>> {
        None
    }

    /// Gets the entity as an `ItemEntity` if it is one.
    fn as_item_entity(self: Arc<Self>) -> Option<Arc<ItemEntity>> {
        None
    }

    /// Gets the entity's rotation as (yaw, pitch) in degrees.
    ///
    /// Yaw is horizontal rotation (0-360), pitch is vertical (-90 to 90).
    fn rotation(&self) -> (f32, f32) {
        (0.0, 0.0)
    }

    /// Gets the eye height for this entity.
    ///
    /// Default implementation returns the eye height from the entity type dimensions.
    /// Override for entities with pose-dependent eye heights (e.g., players).
    fn get_eye_height(&self) -> f64 {
        f64::from(self.entity_type().dimensions.eye_height)
    }

    /// Gets the Y coordinate of the entity's eyes.
    ///
    /// Equivalent to vanilla's `Entity.getEyeY()`.
    fn get_eye_y(&self) -> f64 {
        self.position().y + self.get_eye_height()
    }

    /// Gets the entity's velocity in blocks per tick.
    fn velocity(&self) -> Vector3<f64> {
        Vector3::new(0.0, 0.0, 0.0)
    }

    /// Sets the entity's velocity.
    fn set_velocity(&self, _velocity: Vector3<f64>) {}

    /// Returns true if the entity is on the ground.
    fn on_ground(&self) -> bool {
        false
    }

    /// Sets whether the entity is on the ground.
    fn set_on_ground(&self, _on_ground: bool) {}

    /// Sets the entity's position.
    fn set_position(&self, pos: Vector3<f64>) {
        if let Some(base) = self.base() {
            base.set_position(pos);
        }
    }

    // === Physics Helper Methods ===
    // These mirror vanilla's Entity class methods.

    /// Gets the default gravity for this entity type.
    ///
    /// Override this to specify entity-specific gravity.
    /// Vanilla values: `ItemEntity` = 0.04, `LivingEntity` = 0.08
    fn get_default_gravity(&self) -> f64 {
        0.0
    }

    /// Returns true if gravity is disabled for this entity.
    ///
    /// Override to read from entity data's `no_gravity` field.
    fn is_no_gravity(&self) -> bool {
        false
    }

    /// Gets the current gravity value.
    ///
    /// Returns 0 if `no_gravity` is set, otherwise returns `get_default_gravity()`.
    fn get_gravity(&self) -> f64 {
        if self.is_no_gravity() {
            0.0
        } else {
            self.get_default_gravity()
        }
    }

    /// Applies gravity to the entity's velocity.
    ///
    /// Mirrors vanilla's `Entity.applyGravity()`.
    fn apply_gravity(&self) {
        let gravity = self.get_gravity();
        if gravity != 0.0 {
            let mut vel = self.velocity();
            vel.y -= gravity;
            self.set_velocity(vel);
        }
    }

    /// Moves the entity with collision detection.
    ///
    /// Mirrors vanilla's `Entity.move(MoverType, Vec3)`.
    /// Updates position, `on_ground`, velocity (on collision), and returns collision info.
    fn do_move(&self, mover_type: MoverType) -> Option<MoveResult> {
        let world = self.level()?;
        let velocity = self.velocity();

        // Build physics state
        let mut physics_state = EntityPhysicsState::new(self.position(), self.entity_type());
        physics_state.velocity = velocity;
        physics_state.on_ground = self.on_ground();
        // Most entities don't step up; override for entities that do
        physics_state.max_up_step = 0.0;
        physics_state.is_crouching = false;

        // Perform collision detection and movement
        let collision_world = WorldCollisionProvider::new(&world);
        let result = move_entity(&physics_state, velocity, mover_type, &collision_world);

        // Update entity state
        self.set_position(result.final_position);
        self.set_on_ground(result.on_ground);

        // Vanilla: Entity.move() zeros velocity components on collision.
        // Horizontal collision zeros X/Z individually based on which axis collided.
        // Vertical collision calls Block.updateEntityMovementAfterFallOn which by default zeros Y.
        // (vanilla: Entity.move lines 776-785)
        // TODO: Support block-specific behavior (slime bounce, etc.)
        if result.horizontal_collision {
            let vel = self.velocity();
            self.set_velocity(Vector3::new(
                if result.x_collision { 0.0 } else { vel.x },
                vel.y,
                if result.z_collision { 0.0 } else { vel.z },
            ));
        }
        if result.vertical_collision {
            // Default Block.updateEntityMovementAfterFallOn behavior: zero Y velocity
            let vel = self.velocity();
            self.set_velocity(Vector3::new(vel.x, 0.0, vel.z));
        }

        Some(result)
    }

    /// Spawns an item at this entity's location.
    ///
    /// Mirrors vanilla's `Entity.spawnAtLocation()`. The item spawns at the
    /// entity's position with the given Y offset and has a default pickup delay.
    ///
    /// Returns `None` if the item stack is empty or the entity has no world.
    fn spawn_at_location(
        &self,
        item: ItemStack,
        y_offset: f64,
    ) -> Option<Arc<entities::ItemEntity>> {
        let world = self.level()?;
        let pos = self.position();
        world.spawn_item(Vector3::new(pos.x, pos.y + y_offset, pos.z), item)
    }

    // === Persistence Methods ===
    // These mirror vanilla's Entity.addAdditionalSaveData/readAdditionalSaveData.

    /// Saves type-specific entity data to NBT.
    ///
    /// Called during chunk serialization. Implementors should save all data
    /// needed to restore entity state on load. Base fields (pos, motion,
    /// rotation, uuid, `on_ground`) are handled by the serialization layer.
    ///
    /// Mirrors vanilla's `Entity.addAdditionalSaveData()`.
    fn save_additional(&self, _nbt: &mut NbtCompound) {}

    /// Loads type-specific entity data from NBT.
    ///
    /// Called after entity creation during chunk deserialization. Base fields
    /// are already restored; this handles type-specific data.
    ///
    /// Mirrors vanilla's `Entity.readAdditionalSaveData()`.
    fn load_additional(&self, _nbt: &BaseNbtCompound<'_>) {}

    // === Tick Tracking ===
    // These methods prevent double-ticking when an entity moves between chunks
    // during the same server tick.

    /// Checks if this entity was already ticked during the given server tick.
    ///
    /// This prevents double-ticking when an entity moves to a different chunk
    /// during its tick, and that chunk gets ticked later in the same server tick.
    ///
    /// Returns `true` if already ticked this tick, `false` otherwise.
    fn was_ticked_this_tick(&self, server_tick: i32) -> bool {
        self.base()
            .is_some_and(|b| b.was_ticked_this_tick(server_tick))
    }

    /// Marks this entity as ticked for the given server tick.
    ///
    /// Called by `EntityStorage::tick()` before ticking an entity.
    fn mark_ticked(&self, server_tick: i32) {
        if let Some(base) = self.base() {
            base.mark_ticked(server_tick);
        }
    }
}

/// A trait for living entities that can take damage, heal, and die.
///
/// This trait provides the core functionality for entities that have health,
/// can be damaged, and can die. It's based on Minecraft's `LivingEntity` class.
pub trait LivingEntity: Entity {
    /// Gets the current health of the entity.
    fn get_health(&self) -> f32;

    /// Sets the health of the entity, clamped between 0 and max health.
    fn set_health(&mut self, health: f32);

    /// Gets the maximum health of the entity.
    fn get_max_health(&self) -> f32;

    /// Heals the entity by the specified amount.
    fn heal(&mut self, amount: f32) {
        let current_health = self.get_health();
        if current_health > 0.0 {
            self.set_health(current_health + amount);
        }
    }

    /// Returns true if the entity is dead or dying (health <= 0).
    fn is_dead_or_dying(&self) -> bool {
        self.get_health() <= 0.0
    }

    /// Returns true if the entity is alive (health > 0).
    fn is_alive(&self) -> bool {
        !self.is_dead_or_dying()
    }

    /// Gets the entity's position.
    fn get_position(&self) -> Vector3<f64>;

    /// Gets the absorption amount (extra health from effects like absorption).
    fn get_absorption_amount(&self) -> f32;

    /// Sets the absorption amount.
    fn set_absorption_amount(&mut self, amount: f32);

    /// Gets the entity's armor value.
    fn get_armor_value(&self) -> i32;

    /// Checks if the entity can be affected by potions.
    fn is_affected_by_potions(&self) -> bool {
        true
    }

    /// Checks if the entity is attackable.
    fn attackable(&self) -> bool {
        true
    }

    /// Checks if the entity is currently using an item.
    fn is_using_item(&self) -> bool {
        false
    }

    /// Checks if the entity is blocking with a shield or similar item.
    fn is_blocking(&self) -> bool {
        false
    }

    /// Checks if the entity is fall flying (using elytra).
    fn is_fall_flying(&self) -> bool {
        false
    }

    /// Checks if the entity is sleeping.
    fn is_sleeping(&self) -> bool {
        false
    }

    /// Stops the entity from sleeping.
    fn stop_sleeping(&mut self) {}

    /// Checks if the entity is sprinting.
    fn is_sprinting(&self) -> bool {
        false
    }

    /// Sets whether the entity is sprinting.
    fn set_sprinting(&mut self, sprinting: bool);

    /// Gets the entity's speed attribute value.
    fn get_speed(&self) -> f32;

    /// Sets the entity's speed.
    fn set_speed(&mut self, speed: f32);

    // Equipment methods

    /// Gets a clone of the item in the specified equipment slot.
    ///
    /// Default implementation returns an empty stack.
    fn get_item_by_slot(&self, _slot: EquipmentSlot) -> ItemStack {
        ItemStack::empty()
    }

    /// Gets the main hand item.
    fn get_main_hand_item(&self) -> ItemStack {
        self.get_item_by_slot(EquipmentSlot::MainHand)
    }

    /// Gets the off hand item.
    fn get_off_hand_item(&self) -> ItemStack {
        self.get_item_by_slot(EquipmentSlot::OffHand)
    }

    /// Checks if the main hand slot is empty.
    fn is_main_hand_empty(&self) -> bool {
        self.get_item_by_slot(EquipmentSlot::MainHand).is_empty()
    }

    /// Checks if the off hand slot is empty.
    fn is_off_hand_empty(&self) -> bool {
        self.get_item_by_slot(EquipmentSlot::OffHand).is_empty()
    }
}
