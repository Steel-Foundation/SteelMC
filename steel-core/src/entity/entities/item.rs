//! Item entity implementation (dropped items).
//!
//! `ItemEntity` represents a dropped item in the world. It has physics
//! (gravity, friction), despawns after 5 minutes, and can be picked up
//! by players after a short delay.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Weak};

use crossbeam::atomic::AtomicCell;
use steel_registry::blocks::shapes::AABBd;
use steel_registry::entity_data::DataValue;
use steel_registry::entity_types::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::ItemEntityData;
use steel_utils::UuidExt;
use steel_utils::locks::SyncMutex;
use steel_utils::math::Vector3;
use uuid::Uuid;

use crate::entity::{Entity, EntityLevelCallback, NullEntityCallback, RemovalReason};
use crate::inventory::container::Container;
use crate::physics::MoverType;
use crate::player::Player;
use crate::world::World;

use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_protocol::packets::game::{
    CEntityPositionSync, CMoveEntityPos, CSetEntityMotion, CTakeItemEntity, calc_delta,
};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::BlockPos;

/// Maximum age in ticks before despawn (5 minutes = 6000 ticks).
const LIFETIME: i32 = 6000;

/// Pickup delay set by `set_default_pickup_delay()` (0.5 seconds = 10 ticks).
/// Note: Items spawn with 0 delay by default; this is only used when explicitly set.
const DEFAULT_PICKUP_DELAY: i32 = 10;

/// Pickup delay value meaning "never pickupable".
const INFINITE_PICKUP_DELAY: i32 = 32767;

/// Age value meaning "infinite lifetime" (never despawns).
const INFINITE_LIFETIME: i32 = -32768;

/// Default health (damage resistance).
const DEFAULT_HEALTH: i32 = 5;

/// Gravity applied per tick (blocks/tick^2). Vanilla: `ItemEntity.getDefaultGravity()`
const DEFAULT_GRAVITY: f64 = 0.04;

/// Air/vertical drag multiplier per tick.
const AIR_DRAG: f64 = 0.98;

/// A dropped item entity.
///
/// Mirrors vanilla's `ItemEntity` behavior:
/// - Falls with gravity (0.04 per tick)
/// - Applies friction when on ground (0.98)
/// - Despawns after 5 minutes (6000 ticks)
/// - Has pickup delay before players can collect it
pub struct ItemEntity {
    // === Core Identity ===
    /// Unique network ID for this entity.
    id: i32,
    /// Persistent UUID for this entity.
    uuid: Uuid,

    // === World Reference ===
    /// The world this entity is in. Mirrors vanilla's `Entity.level`.
    world: Weak<World>,

    // === Position & Physics ===
    /// Current position in the world.
    position: SyncMutex<Vector3<f64>>,
    /// Velocity in blocks per tick.
    velocity: SyncMutex<Vector3<f64>>,
    /// Rotation as (yaw, pitch) in degrees. Items have random yaw on spawn.
    rotation: AtomicCell<(f32, f32)>,
    /// Whether the entity is on the ground.
    on_ground: AtomicBool,

    // === Synced Entity Data ===
    /// Entity data containing the `ItemStack`.
    entity_data: SyncMutex<ItemEntityData>,

    // === Timers ===
    /// Age in ticks. Despawns at `LIFETIME` (6000). Special value -32768 = infinite.
    age: AtomicI32,
    /// Per-entity tick counter used for vanilla timing logic.
    ///
    /// Vanilla uses `Entity.tickCount` (always increments) for things like the
    /// `(tickCount + id) % 4 == 0` movement fallback and periodic position sync.
    /// This must not be tied to `age` because `age` can be set to `INFINITE_LIFETIME`.
    tick_count: AtomicI32,
    /// Ticks until pickupable. 0 = can pickup, 32767 = never.
    pickup_delay: AtomicI32,
    /// Health (damage resistance). Item is destroyed when this reaches 0.
    health: AtomicI32,

    // === Lifecycle ===
    /// Whether this entity has been removed.
    removed: AtomicBool,
    /// Callback for entity lifecycle events.
    level_callback: SyncMutex<Arc<dyn EntityLevelCallback>>,

    // === Item-specific ===
    /// UUID of the entity that threw/dropped this item.
    thrower: SyncMutex<Option<Uuid>>,
    /// UUID of the only entity that can pick up this item.
    /// If `None`, any player can pick it up.
    /// Vanilla calls this `target`.
    owner: SyncMutex<Option<Uuid>>,

    // === Network Sync ===
    /// Last velocity sent to clients (for delta detection).
    /// Mirrors vanilla's `ServerEntity.lastSentMovement`.
    last_sent_velocity: SyncMutex<Vector3<f64>>,
    /// Last position sent to clients (for delta detection).
    /// Mirrors vanilla's `ServerEntity.lastSentXyz` fields.
    last_sent_position: SyncMutex<Vector3<f64>>,
    /// Last `on_ground` state sent to clients.
    last_sent_on_ground: AtomicBool,
    /// Whether position/velocity needs to be synced to clients.
    /// Set when velocity changes significantly, like vanilla's `Entity.needsSync`.
    needs_sync: AtomicBool,
}

impl ItemEntity {
    /// Creates a new item entity with an empty item.
    ///
    /// Use `set_item()` to set the actual item after creation, or use `with_item()`.
    #[must_use]
    pub fn new(id: i32, position: Vector3<f64>, world: Weak<World>) -> Self {
        Self::with_item(id, position, ItemStack::empty(), world)
    }

    /// Creates a new item entity with the specified item.
    #[must_use]
    pub fn with_item(id: i32, position: Vector3<f64>, item: ItemStack, world: Weak<World>) -> Self {
        Self::with_item_and_velocity(id, position, item, Vector3::new(0.0, 0.0, 0.0), world)
    }

    /// Creates a new item entity with the specified item and initial velocity.
    ///
    /// Mirrors vanilla's `ItemEntity(Level, double, double, double, ItemStack, double, double, double)`.
    #[must_use]
    pub fn with_item_and_velocity(
        id: i32,
        position: Vector3<f64>,
        item: ItemStack,
        velocity: Vector3<f64>,
        world: Weak<World>,
    ) -> Self {
        // Random yaw rotation for visual variety
        let yaw = rand::random::<f32>() * 360.0;

        let mut entity_data = ItemEntityData::new();
        entity_data.item.set(item);

        Self {
            id,
            uuid: Uuid::new_v4(),
            world,
            position: SyncMutex::new(position),
            velocity: SyncMutex::new(velocity),
            rotation: AtomicCell::new((yaw, 0.0)),
            on_ground: AtomicBool::new(false),
            entity_data: SyncMutex::new(entity_data),
            age: AtomicI32::new(0),
            tick_count: AtomicI32::new(0),
            pickup_delay: AtomicI32::new(0),
            health: AtomicI32::new(DEFAULT_HEALTH),
            removed: AtomicBool::new(false),
            level_callback: SyncMutex::new(Arc::new(NullEntityCallback)),
            thrower: SyncMutex::new(None),
            owner: SyncMutex::new(None),
            last_sent_velocity: SyncMutex::new(velocity),
            last_sent_position: SyncMutex::new(position),
            last_sent_on_ground: AtomicBool::new(false),
            needs_sync: AtomicBool::new(false),
        }
    }

    /// Creates an item entity from saved data with restored base state.
    ///
    /// Used when loading entities from disk. Type-specific data (item, age, etc.)
    /// is restored via `load_additional()` after this constructor.
    #[must_use]
    pub fn from_saved(
        id: i32,
        position: Vector3<f64>,
        uuid: Uuid,
        velocity: Vector3<f64>,
        rotation: (f32, f32),
        on_ground: bool,
        world: Weak<World>,
    ) -> Self {
        Self {
            id,
            uuid,
            world,
            position: SyncMutex::new(position),
            velocity: SyncMutex::new(velocity),
            rotation: AtomicCell::new(rotation),
            on_ground: AtomicBool::new(on_ground),
            entity_data: SyncMutex::new(ItemEntityData::new()),
            age: AtomicI32::new(0),
            tick_count: AtomicI32::new(0),
            pickup_delay: AtomicI32::new(0),
            health: AtomicI32::new(DEFAULT_HEALTH),
            removed: AtomicBool::new(false),
            level_callback: SyncMutex::new(Arc::new(NullEntityCallback)),
            thrower: SyncMutex::new(None),
            owner: SyncMutex::new(None),
            last_sent_velocity: SyncMutex::new(velocity),
            last_sent_position: SyncMutex::new(position),
            last_sent_on_ground: AtomicBool::new(on_ground),
            needs_sync: AtomicBool::new(false),
        }
    }

    // === Item Access ===

    /// Gets a clone of the item stack.
    #[must_use]
    pub fn get_item(&self) -> ItemStack {
        self.entity_data.lock().item.get().clone()
    }

    /// Sets the item stack.
    pub fn set_item(&self, item: ItemStack) {
        self.entity_data.lock().item.set(item);
    }

    // === Position & Physics ===
    // Note: set_position, set_velocity, set_on_ground are implemented
    // via the Entity trait.

    /// Gets whether the entity is on the ground.
    #[must_use]
    pub fn is_on_ground(&self) -> bool {
        self.on_ground.load(Ordering::Relaxed)
    }

    // === Timers ===

    /// Gets the current age in ticks.
    #[must_use]
    pub fn get_age(&self) -> i32 {
        self.age.load(Ordering::Relaxed)
    }

    /// Sets the age in ticks.
    pub fn set_age(&self, age: i32) {
        self.age.store(age, Ordering::Relaxed);
    }

    /// Returns this entity's internal tick counter.
    ///
    /// This mirrors vanilla `Entity.tickCount` and always increments, even when
    /// `age` is set to `INFINITE_LIFETIME`.
    #[must_use]
    pub fn get_tick_count(&self) -> i32 {
        self.tick_count.load(Ordering::Relaxed)
    }

    /// Sets the entity to never despawn.
    pub fn set_unlimited_lifetime(&self) {
        self.age.store(INFINITE_LIFETIME, Ordering::Relaxed);
    }

    /// Gets the pickup delay in ticks.
    #[must_use]
    pub fn get_pickup_delay(&self) -> i32 {
        self.pickup_delay.load(Ordering::Relaxed)
    }

    /// Sets the default pickup delay (10 ticks = 0.5 seconds).
    pub fn set_default_pickup_delay(&self) {
        self.pickup_delay
            .store(DEFAULT_PICKUP_DELAY, Ordering::Relaxed);
    }

    /// Sets the pickup delay to zero (immediately pickupable).
    pub fn set_no_pickup_delay(&self) {
        self.pickup_delay.store(0, Ordering::Relaxed);
    }

    /// Sets the item to never be pickupable.
    pub fn set_never_pickup(&self) {
        self.pickup_delay
            .store(INFINITE_PICKUP_DELAY, Ordering::Relaxed);
    }

    /// Sets a custom pickup delay in ticks.
    pub fn set_pickup_delay(&self, delay: i32) {
        self.pickup_delay.store(delay, Ordering::Relaxed);
    }

    /// Returns true if the item has a pickup delay (cannot be picked up yet).
    #[must_use]
    pub fn has_pickup_delay(&self) -> bool {
        self.pickup_delay.load(Ordering::Relaxed) > 0
    }

    // === Health ===

    /// Gets the health (damage resistance).
    #[must_use]
    pub fn get_health(&self) -> i32 {
        self.health.load(Ordering::Relaxed)
    }

    /// Sets the health.
    pub fn set_health(&self, health: i32) {
        self.health.store(health, Ordering::Relaxed);
    }

    // === Thrower ===

    /// Sets the entity that threw/dropped this item.
    pub fn set_thrower(&self, uuid: Uuid) {
        *self.thrower.lock() = Some(uuid);
    }

    /// Gets the UUID of the entity that threw/dropped this item.
    #[must_use]
    pub fn get_thrower(&self) -> Option<Uuid> {
        *self.thrower.lock()
    }

    // === Owner ===

    /// Sets the owner (the only entity that can pick up this item).
    ///
    /// Pass `None` to allow any player to pick it up.
    /// Vanilla calls this `target`.
    pub fn set_owner(&self, uuid: Option<Uuid>) {
        *self.owner.lock() = uuid;
    }

    /// Gets the owner UUID (the only entity that can pick up this item).
    ///
    /// Returns `None` if any player can pick it up.
    #[must_use]
    pub fn get_owner(&self) -> Option<Uuid> {
        *self.owner.lock()
    }

    // === Pickup ===

    /// Attempts to have a player pick up this item.
    ///
    /// Returns `true` if the item was fully picked up (and the entity should be removed),
    /// `false` if pickup failed or was only partial.
    ///
    /// Mirrors vanilla's `ItemEntity.playerTouch(Player)`.
    pub fn try_pickup(&self, player: &Arc<Player>) -> bool {
        // Check pickup delay
        if self.has_pickup_delay() {
            return false;
        }

        // Check owner restriction
        if let Some(owner_uuid) = self.get_owner()
            && owner_uuid != player.gameprofile.id
        {
            return false;
        }

        // Get the item and try to add to inventory
        let mut item = self.get_item();
        let original_count = item.count();

        // Try to add to player's inventory
        let added = player.inventory.lock().add(&mut item);

        // If nothing was added, bail out
        if item.count() == original_count {
            return false;
        }

        // Calculate how many items were picked up
        let picked_up_count = original_count - item.count();

        // Send the take animation packet to nearby players
        if let Some(world) = self.level() {
            let pos = self.position();
            let chunk_pos = steel_utils::ChunkPos::new((pos.x as i32) >> 4, (pos.z as i32) >> 4);

            let take_packet = CTakeItemEntity::new(self.id, player.id, picked_up_count);
            world.broadcast_to_nearby(chunk_pos, take_packet, None);
        }

        // Update or remove the item entity
        if added {
            // Fully picked up - mark for removal
            self.set_removed(RemovalReason::Discarded);
            true
        } else {
            // Partial pickup - update the remaining item
            self.set_item(item);
            false
        }
    }

    // === Merging ===

    /// Returns true if this item entity can be merged with others.
    ///
    /// Mirrors vanilla's `ItemEntity.isMergeable()`.
    /// An item is mergeable if:
    /// - It's not removed
    /// - It doesn't have infinite pickup delay (32767)
    /// - It doesn't have infinite lifetime (-32768)
    /// - Its age is less than the despawn threshold (6000)
    /// - Its count is less than max stack size
    #[must_use]
    pub fn is_mergeable(&self) -> bool {
        let item = self.get_item();
        !self.is_removed()
            && self.pickup_delay.load(Ordering::Relaxed) != INFINITE_PICKUP_DELAY
            && self.age.load(Ordering::Relaxed) != INFINITE_LIFETIME
            && self.age.load(Ordering::Relaxed) < LIFETIME
            && item.count() < item.max_stack_size()
    }

    /// Checks if two item stacks can be merged together.
    ///
    /// Mirrors vanilla's `ItemEntity.areMergeable()`.
    /// Returns true if the items are the same type with the same components,
    /// and their combined count wouldn't exceed max stack size.
    #[must_use]
    pub fn are_mergeable(this_stack: &ItemStack, other_stack: &ItemStack) -> bool {
        // Combined count must not exceed max stack size
        if other_stack.count() + this_stack.count() > other_stack.max_stack_size() {
            return false;
        }
        // Must be the same item with the same components
        ItemStack::is_same_item_same_components(this_stack, other_stack)
    }

    /// Attempts to merge with another item entity.
    ///
    /// Mirrors vanilla's `ItemEntity.tryToMerge()`.
    /// The item with fewer items is merged into the one with more.
    fn try_to_merge(&self, other: &ItemEntity) {
        let this_stack = self.get_item();
        let other_stack = other.get_item();

        // Both items must have the same owner (target)
        if self.get_owner() != other.get_owner() {
            return;
        }

        if !Self::are_mergeable(&this_stack, &other_stack) {
            return;
        }

        // Merge smaller stack into larger stack
        if other_stack.count() < this_stack.count() {
            Self::merge_stacks(self, &this_stack, other, &other_stack);
        } else {
            Self::merge_stacks(other, &other_stack, self, &this_stack);
        }
    }

    /// Merges the `from_item`'s stack into the `to_item`'s stack.
    ///
    /// Mirrors vanilla's `ItemEntity.merge(ItemEntity, ItemStack, ItemEntity, ItemStack)`.
    fn merge_stacks(
        to_item: &ItemEntity,
        to_stack: &ItemStack,
        from_item: &ItemEntity,
        from_stack: &ItemStack,
    ) {
        // Calculate how many items to transfer
        let max_count = to_stack.max_stack_size();
        let space_available = max_count - to_stack.count();
        let transfer_count = space_available.min(from_stack.count());

        // Create new stacks
        let new_to_stack = to_stack.copy_with_count(to_stack.count() + transfer_count);
        let mut new_from_stack = from_stack.clone();
        new_from_stack.shrink(transfer_count);

        // Update the destination item
        to_item.set_item(new_to_stack);

        // Pickup delay is the max of both (so merged items don't become instantly pickable)
        let new_pickup_delay = to_item
            .pickup_delay
            .load(Ordering::Relaxed)
            .max(from_item.pickup_delay.load(Ordering::Relaxed));
        to_item
            .pickup_delay
            .store(new_pickup_delay, Ordering::Relaxed);

        // Age is the min of both (so merged items don't despawn prematurely)
        let new_age = to_item
            .age
            .load(Ordering::Relaxed)
            .min(from_item.age.load(Ordering::Relaxed));
        to_item.age.store(new_age, Ordering::Relaxed);

        // Update or remove the source item
        if new_from_stack.is_empty() {
            from_item.set_removed(RemovalReason::Discarded);
        } else {
            from_item.set_item(new_from_stack);
        }
    }

    /// Attempts to merge this item with nearby item entities.
    ///
    /// Mirrors vanilla's `ItemEntity.mergeWithNeighbours()`.
    /// Searches for other mergeable item entities within 0.5 blocks horizontally
    /// and attempts to merge with them.
    pub fn merge_with_neighbours(&self, world: &World) {
        if !self.is_mergeable() {
            return;
        }

        // Search area: 0.5 blocks horizontal, 0 vertical (vanilla uses inflate(0.5, 0.0, 0.5))
        let search_box = self.bounding_box().inflate_xyz(0.5, 0.0, 0.5);

        // Get all entities in the search area
        for entity in world.get_entities_in_aabb(&search_box) {
            // Skip self
            if entity.id() == self.id {
                continue;
            }

            // Try to get as ItemEntity
            if let Some(other_item) = entity.as_item_entity() {
                // Double-check mergability (might have changed)
                if other_item.is_mergeable() {
                    self.try_to_merge(&other_item);

                    // If we've been removed (merged into other), stop
                    if self.is_removed() {
                        break;
                    }
                }
            }
        }
    }

    // === Network Sync ===

    /// Checks if velocity should be synced and returns the packet if needed.
    ///
    /// Vanilla syncs velocity when:
    /// - Velocity changed by more than 1e-7 squared distance
    /// - OR velocity became zero (to stop client-side prediction)
    fn check_velocity_sync(&self) -> Option<CSetEntityMotion> {
        let current = self.velocity();
        let last_sent = *self.last_sent_velocity.lock();

        let diff_sq = (current.x - last_sent.x).powi(2)
            + (current.y - last_sent.y).powi(2)
            + (current.z - last_sent.z).powi(2);

        // Sync if velocity changed significantly, or if it went to zero
        // (vanilla: ServerEntity.sendChanges lines 170-172)
        let should_sync = diff_sq > 1.0e-7
            || (diff_sq > 0.0 && current.x == 0.0 && current.y == 0.0 && current.z == 0.0);

        if should_sync {
            *self.last_sent_velocity.lock() = current;
            Some(CSetEntityMotion::new(
                self.id, current.x, current.y, current.z,
            ))
        } else {
            None
        }
    }

    /// Checks if position should be synced and returns the appropriate packet.
    ///
    /// Uses delta encoding (`CMoveEntityPos`) for small movements, and falls back
    /// to absolute position sync (`CEntityPositionSync`) when:
    /// - Delta is too large for i16 encoding
    /// - On-ground state changed
    /// - Periodic full sync (every 60 ticks based on `tick_count`)
    fn check_position_sync(&self, tick_count: i32) -> Option<PositionSyncPacket> {
        let current_pos = self.position();
        let last_sent = *self.last_sent_position.lock();
        let current_on_ground = self.on_ground();
        let last_on_ground = self.last_sent_on_ground.load(Ordering::Relaxed);

        // Check if position changed enough to warrant sync
        // Vanilla threshold: 7.6293945E-6 (TOLERANCE_LEVEL_POSITION)
        let diff_sq = (current_pos.x - last_sent.x).powi(2)
            + (current_pos.y - last_sent.y).powi(2)
            + (current_pos.z - last_sent.z).powi(2);

        let position_changed = diff_sq >= 7.629_394_5e-6;
        let on_ground_changed = current_on_ground != last_on_ground;
        // Vanilla uses tickCount % 60 for periodic full position sync (FORCED_POS_UPDATE_PERIOD)
        let force_periodic_sync = tick_count % 60 == 0;

        // Vanilla: boolean pos = positionChanged || this.tickCount % 60 == 0;
        // We sync if position changed, on_ground changed, or periodic
        if !position_changed && !on_ground_changed && !force_periodic_sync {
            return None;
        }

        // Try delta encoding first
        let dx = calc_delta(current_pos.x, last_sent.x);
        let dy = calc_delta(current_pos.y, last_sent.y);
        let dz = calc_delta(current_pos.z, last_sent.z);

        // Use full sync if delta overflow or on-ground changed or periodic
        // (vanilla: ServerEntity.sendChanges line 123)
        let use_full_sync = on_ground_changed
            || force_periodic_sync
            || dx.is_none()
            || dy.is_none()
            || dz.is_none();

        self.last_sent_on_ground
            .store(current_on_ground, Ordering::Relaxed);

        if use_full_sync {
            // Full sync: client sets position directly, so store current_pos
            *self.last_sent_position.lock() = current_pos;

            let vel = self.velocity();
            // NOTE: We do NOT update last_sent_velocity here because the client
            // ignores the velocity field in CEntityPositionSync for non-authoritative
            // entities (like items). The velocity sync is handled separately by
            // check_velocity_sync() which sends CSetEntityMotion.

            let (yaw, pitch) = self.rotation.load();
            Some(PositionSyncPacket::Full(CEntityPositionSync {
                entity_id: self.id,
                x: current_pos.x,
                y: current_pos.y,
                z: current_pos.z,
                velocity_x: vel.x,
                velocity_y: vel.y,
                velocity_z: vel.z,
                yaw,
                pitch,
                on_ground: current_on_ground,
            }))
        } else {
            // Delta sync: store the actual current position as base.
            // Vanilla stores the actual position, not the decoded position.
            // This works because encode() is deterministic - both server and client
            // compute the same encoded values.
            let dx = dx.expect("delta dx missing in delta position sync");
            let dy = dy.expect("delta dy missing in delta position sync");
            let dz = dz.expect("delta dz missing in delta position sync");

            *self.last_sent_position.lock() = current_pos;

            Some(PositionSyncPacket::Delta(CMoveEntityPos {
                entity_id: self.id,
                dx,
                dy,
                dz,
                on_ground: current_on_ground,
            }))
        }
    }
}

/// Position sync packet variants.
enum PositionSyncPacket {
    /// Delta-encoded position update (for small movements).
    Delta(CMoveEntityPos),
    /// Full absolute position sync (for large movements or periodic sync).
    Full(CEntityPositionSync),
}

impl Entity for ItemEntity {
    fn entity_type(&self) -> EntityTypeRef {
        vanilla_entities::ITEM
    }

    fn id(&self) -> i32 {
        self.id
    }

    fn uuid(&self) -> Uuid {
        self.uuid
    }

    fn position(&self) -> Vector3<f64> {
        *self.position.lock()
    }

    fn bounding_box(&self) -> AABBd {
        let pos = self.position();
        let dims = self.entity_type().dimensions;
        let half_width = f64::from(dims.width) / 2.0;
        let height = f64::from(dims.height);
        AABBd {
            min_x: pos.x - half_width,
            min_y: pos.y,
            min_z: pos.z - half_width,
            max_x: pos.x + half_width,
            max_y: pos.y + height,
            max_z: pos.z + half_width,
        }
    }

    fn tick(&self) {
        // Vanilla: `Entity.tickCount` increments every tick regardless of item age/lifetime.
        let tick_count = self.tick_count.fetch_add(1, Ordering::Relaxed) + 1;

        // Check if item is empty
        if self.get_item().is_empty() {
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        // Decrement pickup delay
        let pickup_delay = self.pickup_delay.load(Ordering::Relaxed);
        if pickup_delay > 0 && pickup_delay != INFINITE_PICKUP_DELAY {
            self.pickup_delay.fetch_sub(1, Ordering::Relaxed);
        }

        // Increment age and check for despawn
        let age = self.age.load(Ordering::Relaxed);
        if age != INFINITE_LIFETIME {
            let new_age = self.age.fetch_add(1, Ordering::Relaxed) + 1;
            if new_age >= LIFETIME {
                self.set_removed(RemovalReason::Discarded);
                return;
            }
        }

        // Store old position for merge rate calculation (vanilla: xo, yo, zo)
        let old_pos = self.position();
        // Store old movement for needsSync check (vanilla: ItemEntity.tick line 98)
        let old_movement = self.velocity();
        // Store old on_ground to detect changes (triggers immediate sync)
        let old_on_ground = self.on_ground();

        // TODO: Handle water/lava movement (setUnderwaterMovement, setUnderLavaMovement)
        // For now, just apply gravity
        self.apply_gravity();

        // Vanilla optimization: skip physics when at rest on ground.
        // Only process physics if:
        // 1. Not on ground, OR
        // 2. Has significant horizontal movement, OR
        // 3. Every 4th tick (for items that might need to fall through opened trapdoors, etc.)
        // (vanilla: ItemEntity.tick line 121)
        let vel = self.velocity();
        let horizontal_movement_sq = vel.x * vel.x + vel.z * vel.z;
        let should_move =
            !self.on_ground() || horizontal_movement_sq > 1.0e-5 || (tick_count + self.id) % 4 == 0;

        if should_move {
            // Move with collision detection (do_move handles velocity zeroing on collision)
            if let Some(result) = self.do_move(MoverType::SelfMovement) {
                // Get world for block queries
                if let Some(world) = self.level() {
                    // Apply friction (vanilla: ItemEntity.tick line 125-128)
                    let friction = if result.on_ground {
                        // Block below that affects movement (0.999999F offset like vanilla)
                        let pos = self.position();
                        let block_pos = BlockPos::new(
                            pos.x.floor() as i32,
                            (pos.y - 0.999_999).floor() as i32,
                            pos.z.floor() as i32,
                        );
                        let block_state = world.get_block_state(&block_pos);
                        f64::from(block_state.get_block().config.friction) * 0.98
                    } else {
                        0.98 // Air friction
                    };

                    let mut velocity = self.velocity();
                    velocity.x *= friction;
                    velocity.z *= friction;
                    velocity.y *= AIR_DRAG;

                    // Bounce when landing on ground (vanilla: ItemEntity.tick lines 145-149)
                    if result.on_ground && velocity.y < 0.0 {
                        velocity.y *= -0.5;
                    }

                    self.set_velocity(velocity);
                }
            }
        }

        // Item merging (vanilla: ItemEntity.tick lines 152-156)
        // Merge rate depends on whether the item moved to a different block
        let current_pos = self.position();
        let moved_block = old_pos.x.floor() as i32 != current_pos.x.floor() as i32
            || old_pos.y.floor() as i32 != current_pos.y.floor() as i32
            || old_pos.z.floor() as i32 != current_pos.z.floor() as i32;
        let merge_rate = if moved_block { 2 } else { 40 };

        if tick_count % merge_rate == 0
            && self.is_mergeable()
            && let Some(world) = self.level()
        {
            self.merge_with_neighbours(&world);
        }

        // Check if velocity changed significantly -> set needsSync (vanilla: ItemEntity.tick lines 160-164)
        // Vanilla: if (getDeltaMovement().subtract(oldMovement).lengthSqr() > 0.01) needsSync = true
        let new_movement = self.velocity();
        let diff = Vector3::new(
            new_movement.x - old_movement.x,
            new_movement.y - old_movement.y,
            new_movement.z - old_movement.z,
        );
        let diff_sq = diff.x * diff.x + diff.y * diff.y + diff.z * diff.z;
        if diff_sq > 0.01 {
            self.needs_sync.store(true, Ordering::Relaxed);
        }

        // Also set needsSync when on_ground changes - this ensures immediate sync
        // when the item lands or becomes airborne, preventing client desync
        if self.on_ground() != old_on_ground {
            self.needs_sync.store(true, Ordering::Relaxed);
        }
    }

    fn send_changes(&self, tick_count: i32) {
        let Some(world) = self.level() else {
            return;
        };

        let update_interval = self.entity_type().update_interval; // 20 for items
        let needs_sync = self.needs_sync.load(Ordering::Relaxed);

        // Only send updates on the update interval OR when needsSync is set
        // (vanilla: ServerEntity.sendChanges line 97)
        if tick_count % update_interval != 0 && !needs_sync {
            return;
        }

        let current_pos = self.position();

        // Determine chunk for broadcasting
        let chunk_pos =
            steel_utils::ChunkPos::new((current_pos.x as i32) >> 4, (current_pos.z as i32) >> 4);

        // Vanilla sends velocity BEFORE position (ServerEntity.sendChanges lines 168-182).
        // Items have trackDelta=true, so we ALWAYS check velocity when in the update window.
        //
        // CRITICAL: The client ignores velocity in CEntityPositionSync for non-authoritative
        // entities (like items). The client runs its own physics simulation and accumulates
        // gravity in deltaMovement. We MUST send CSetEntityMotion to override the client's
        // deltaMovement, otherwise the client's accumulated gravity causes visual desync.
        if let Some(vel_packet) = self.check_velocity_sync() {
            world.broadcast_to_nearby(chunk_pos, vel_packet, None);
        }

        // Send position update if needed (vanilla: ServerEntity.sendChanges line 182)
        if let Some(packet) = self.check_position_sync(tick_count) {
            match &packet {
                PositionSyncPacket::Delta(p) => {
                    world.broadcast_to_nearby(chunk_pos, p.clone(), None);
                }
                PositionSyncPacket::Full(p) => {
                    world.broadcast_to_nearby(chunk_pos, p.clone(), None);
                }
            }
        }

        // Clear needsSync after processing (vanilla: ServerEntity.sendChanges line 193)
        self.needs_sync.store(false, Ordering::Relaxed);
    }

    fn get_default_gravity(&self) -> f64 {
        DEFAULT_GRAVITY
    }

    fn is_no_gravity(&self) -> bool {
        *self.entity_data.lock().no_gravity.get()
    }

    fn level(&self) -> Option<Arc<World>> {
        self.world.upgrade()
    }

    fn as_item_entity(self: Arc<Self>) -> Option<Arc<ItemEntity>> {
        Some(self)
    }

    fn pack_dirty_entity_data(&self) -> Option<Vec<DataValue>> {
        self.entity_data.lock().pack_dirty()
    }

    fn pack_all_entity_data(&self) -> Vec<DataValue> {
        self.entity_data.lock().pack_all()
    }

    fn is_removed(&self) -> bool {
        self.removed.load(Ordering::Relaxed)
    }

    fn set_removed(&self, reason: RemovalReason) {
        if !self.removed.swap(true, Ordering::AcqRel) {
            self.level_callback.lock().on_remove(reason);
        }
    }

    fn set_level_callback(&self, callback: Arc<dyn EntityLevelCallback>) {
        *self.level_callback.lock() = callback;
    }

    fn rotation(&self) -> (f32, f32) {
        self.rotation.load()
    }

    fn velocity(&self) -> Vector3<f64> {
        *self.velocity.lock()
    }

    fn set_velocity(&self, velocity: Vector3<f64>) {
        *self.velocity.lock() = velocity;
    }

    fn on_ground(&self) -> bool {
        self.on_ground.load(Ordering::Relaxed)
    }

    fn set_on_ground(&self, on_ground: bool) {
        self.on_ground.store(on_ground, Ordering::Relaxed);
    }

    fn set_position(&self, pos: Vector3<f64>) {
        let old_pos = {
            let mut position = self.position.lock();
            let old = *position;
            *position = pos;
            old
        };
        // Notify callback of movement
        self.level_callback.lock().on_move(old_pos, pos);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        // Match vanilla's ItemEntity.addAdditionalSaveData
        nbt.insert("Health", self.health.load(Ordering::Relaxed) as i16);
        nbt.insert("Age", self.age.load(Ordering::Relaxed) as i16);
        nbt.insert(
            "PickupDelay",
            self.pickup_delay.load(Ordering::Relaxed) as i16,
        );

        if let Some(thrower) = self.get_thrower() {
            nbt.insert("Thrower", NbtTag::IntArray(thrower.to_int_array().to_vec()));
        }
        if let Some(owner) = self.get_owner() {
            nbt.insert("Owner", NbtTag::IntArray(owner.to_int_array().to_vec()));
        }

        let item = self.get_item();
        if !item.is_empty() {
            nbt.insert("Item", item.to_nbt_tag());
        }
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        // Convert to view type to access accessor methods
        let nbt: NbtCompoundView<'_, '_> = nbt.into();

        // Match vanilla's ItemEntity.readAdditionalSaveData
        if let Some(health) = nbt.short("Health") {
            self.health.store(i32::from(health), Ordering::Relaxed);
        }
        if let Some(age) = nbt.short("Age") {
            self.age.store(i32::from(age), Ordering::Relaxed);
        }
        if let Some(pickup_delay) = nbt.short("PickupDelay") {
            self.pickup_delay
                .store(i32::from(pickup_delay), Ordering::Relaxed);
        }

        if let Some(thrower_arr) = nbt.int_array("Thrower")
            && let Some(uuid) = Uuid::from_int_array(&thrower_arr)
        {
            *self.thrower.lock() = Some(uuid);
        }
        if let Some(owner_arr) = nbt.int_array("Owner")
            && let Some(uuid) = Uuid::from_int_array(&owner_arr)
        {
            *self.owner.lock() = Some(uuid);
        }

        if let Some(item_tag) = nbt.compound("Item")
            && let Some(item) = ItemStack::from_borrowed_compound(&item_tag)
        {
            self.entity_data.lock().item.set(item);
        }

        // Vanilla behavior: discard if item is empty after load
        if self.get_item().is_empty() {
            self.set_removed(RemovalReason::Discarded);
        }
    }
}
