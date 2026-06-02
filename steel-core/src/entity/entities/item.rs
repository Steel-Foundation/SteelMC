//! Item entity implementation (dropped items).
//!
//! `ItemEntity` represents a dropped item in the world. It has physics
//! (gravity, friction), despawns after 5 minutes, and can be picked up
//! by players after a short delay.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::ItemEntityData;
use steel_utils::UuidExt;
use steel_utils::locks::SyncMutex;
use uuid::Uuid;

use crate::entity::damage::DamageSource;

use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityBaseState, EntityFluidContact,
    EntityPositionSyncState, EntitySyncedData, RemovalReason,
};
use crate::inventory::container::Container;
use crate::physics::MoverType;
use crate::player::Player;
use crate::world::World;

use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_protocol::packets::game::{
    CEntityPositionSync, CMoveEntityPos, CSetEntityMotion, CTakeItemEntity,
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
const FLUID_VERTICAL_NUDGE: f64 = 5.0e-4;
const ITEM_FLUID_HEIGHT_THRESHOLD: f64 = 0.1;
const ITEM_WATER_DRAG: f64 = 0.99;
const ITEM_LAVA_DRAG: f64 = 0.95;

/// Mutable item-specific state that changes during item ticks, pickup, damage,
/// merging, and save/load.
struct ItemEntityState {
    /// Age in ticks. Despawns at `LIFETIME` (6000). Special value -32768 = infinite.
    age: i32,
    /// Per-entity tick counter used for vanilla timing logic.
    ///
    /// Vanilla uses `Entity.tickCount` (always increments) for things like the
    /// `(tickCount + id) % 4 == 0` movement fallback and periodic position sync.
    /// This must not be tied to `age` because `age` can be set to `INFINITE_LIFETIME`.
    tick_count: i32,
    /// Ticks until pickupable. 0 = can pickup, 32767 = never.
    pickup_delay: i32,
    /// Health (damage resistance). Item is destroyed when this reaches 0.
    health: i32,
    /// UUID of the entity that threw/dropped this item.
    thrower: Option<Uuid>,
    /// UUID of the only entity that can pick up this item.
    /// If `None`, any player can pick it up. Vanilla calls this `target`.
    owner: Option<Uuid>,
}

impl ItemEntityState {
    const fn new() -> Self {
        Self {
            age: 0,
            tick_count: 0,
            pickup_delay: 0,
            health: DEFAULT_HEALTH,
            thrower: None,
            owner: None,
        }
    }
}

/// Last sent movement state for item entity tracking.
struct ItemEntitySyncState {
    position: EntityPositionSyncState,
    /// Last velocity sent to clients (for delta detection).
    /// Mirrors vanilla's `ServerEntity.lastSentMovement`.
    last_sent_velocity: DVec3,
    /// Whether position/velocity needs to be synced to clients.
    /// Set when velocity changes significantly, like vanilla's `Entity.needsSync`.
    needs_sync: bool,
}

impl ItemEntitySyncState {
    const fn new(position: DVec3, velocity: DVec3, on_ground: bool) -> Self {
        Self {
            position: EntityPositionSyncState::new(position, on_ground),
            last_sent_velocity: velocity,
            needs_sync: false,
        }
    }
}

/// A dropped item entity.
///
/// Mirrors vanilla's `ItemEntity` behavior:
/// - Falls with gravity (0.04 per tick)
/// - Applies friction when on ground (0.98)
/// - Despawns after 5 minutes (6000 ticks)
/// - Has pickup delay before players can collect it
pub struct ItemEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,

    // === Synced Entity Data ===
    /// Entity data containing the `ItemStack`.
    entity_data: SyncMutex<ItemEntityData>,

    /// Item-specific mutable state.
    item_state: SyncMutex<ItemEntityState>,
    /// Movement sync state used by tracking.
    sync_state: SyncMutex<ItemEntitySyncState>,
}

impl ItemEntity {
    /// Creates a new item entity with an empty item.
    ///
    /// Use `set_item()` to set the actual item after creation, or use `with_item()`.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::with_item(id, position, ItemStack::empty(), world)
    }

    /// Creates a new item entity with the specified item.
    #[must_use]
    pub fn with_item(id: i32, position: DVec3, item: ItemStack, world: Weak<World>) -> Self {
        Self::with_item_and_velocity(id, position, item, DVec3::new(0.0, 0.0, 0.0), world)
    }

    /// Creates a new item entity with the specified item and initial velocity.
    ///
    /// Mirrors vanilla's `ItemEntity(Level, double, double, double, ItemStack, double, double, double)`.
    #[must_use]
    pub fn with_item_and_velocity(
        id: i32,
        position: DVec3,
        item: ItemStack,
        velocity: DVec3,
        world: Weak<World>,
    ) -> Self {
        // Random yaw rotation for visual variety
        let yaw = rand::random::<f32>() * 360.0;

        let mut entity_data = ItemEntityData::new();
        entity_data.item.set(item);

        Self {
            base: EntityBase::new_with_state(
                id,
                EntityBaseState::new(position, vanilla_entities::ITEM.dimensions)
                    .with_velocity(velocity)
                    .with_rotation((yaw, 0.0)),
                world,
            ),
            entity_data: SyncMutex::new(entity_data),
            item_state: SyncMutex::new(ItemEntityState::new()),
            sync_state: SyncMutex::new(ItemEntitySyncState::new(position, velocity, false)),
        }
    }

    /// Creates an item entity from saved data with restored base state.
    ///
    /// Used when loading entities from disk. Type-specific data (item, age, etc.)
    /// is restored via `load_additional()` after this constructor.
    #[must_use]
    pub fn from_saved(load: EntityBaseLoad) -> Self {
        let position = load.position;
        let velocity = load.velocity;
        let on_ground = load.on_ground;
        Self {
            base: EntityBase::from_load(load, vanilla_entities::ITEM.dimensions),
            entity_data: SyncMutex::new(ItemEntityData::new()),
            item_state: SyncMutex::new(ItemEntityState::new()),
            sync_state: SyncMutex::new(ItemEntitySyncState::new(position, velocity, on_ground)),
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

    // === Timers ===

    /// Gets the current age in ticks.
    #[must_use]
    pub fn get_age(&self) -> i32 {
        self.item_state.lock().age
    }

    /// Sets the age in ticks.
    pub fn set_age(&self, age: i32) {
        self.item_state.lock().age = age;
    }

    /// Returns this entity's internal tick counter.
    ///
    /// This mirrors vanilla `Entity.tickCount` and always increments, even when
    /// `age` is set to `INFINITE_LIFETIME`.
    #[must_use]
    pub fn get_tick_count(&self) -> i32 {
        self.item_state.lock().tick_count
    }

    /// Sets the entity to never despawn.
    pub fn set_unlimited_lifetime(&self) {
        self.item_state.lock().age = INFINITE_LIFETIME;
    }

    /// Gets the pickup delay in ticks.
    #[must_use]
    pub fn get_pickup_delay(&self) -> i32 {
        self.item_state.lock().pickup_delay
    }

    /// Sets the default pickup delay (10 ticks = 0.5 seconds).
    pub fn set_default_pickup_delay(&self) {
        self.item_state.lock().pickup_delay = DEFAULT_PICKUP_DELAY;
    }

    /// Sets the pickup delay to zero (immediately pickupable).
    pub fn set_no_pickup_delay(&self) {
        self.item_state.lock().pickup_delay = 0;
    }

    /// Sets the item to never be pickupable.
    pub fn set_never_pickup(&self) {
        self.item_state.lock().pickup_delay = INFINITE_PICKUP_DELAY;
    }

    /// Sets a custom pickup delay in ticks.
    pub fn set_pickup_delay(&self, delay: i32) {
        self.item_state.lock().pickup_delay = delay;
    }

    /// Returns true if the item has a pickup delay (cannot be picked up yet).
    #[must_use]
    pub fn has_pickup_delay(&self) -> bool {
        self.item_state.lock().pickup_delay > 0
    }

    // === Health ===

    /// Gets the health (damage resistance).
    #[must_use]
    pub fn get_health(&self) -> i32 {
        self.item_state.lock().health
    }

    /// Sets the health.
    pub fn set_health(&self, health: i32) {
        self.item_state.lock().health = health;
    }

    // === Thrower ===

    /// Sets the entity that threw/dropped this item.
    pub fn set_thrower(&self, uuid: Uuid) {
        self.item_state.lock().thrower = Some(uuid);
    }

    /// Gets the UUID of the entity that threw/dropped this item.
    #[must_use]
    pub fn get_thrower(&self) -> Option<Uuid> {
        self.item_state.lock().thrower
    }

    // === Owner ===

    /// Sets the owner (the only entity that can pick up this item).
    ///
    /// Pass `None` to allow any player to pick it up.
    /// Vanilla calls this `target`.
    pub fn set_owner(&self, uuid: Option<Uuid>) {
        self.item_state.lock().owner = uuid;
    }

    /// Gets the owner UUID (the only entity that can pick up this item).
    ///
    /// Returns `None` if any player can pick it up.
    #[must_use]
    pub fn get_owner(&self) -> Option<Uuid> {
        self.item_state.lock().owner
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
            let chunk_pos = steel_utils::ChunkPos::from_entity_pos(pos);

            let take_packet = CTakeItemEntity::new(self.id(), player.id(), picked_up_count);
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
        let state = self.item_state.lock();
        !self.is_removed()
            && state.pickup_delay != INFINITE_PICKUP_DELAY
            && state.age != INFINITE_LIFETIME
            && state.age < LIFETIME
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
        // Age is the min of both (so merged items don't despawn prematurely).
        let (from_pickup_delay, from_age) = {
            let state = from_item.item_state.lock();
            (state.pickup_delay, state.age)
        };
        {
            let mut state = to_item.item_state.lock();
            state.pickup_delay = state.pickup_delay.max(from_pickup_delay);
            state.age = state.age.min(from_age);
        }

        // Update or remove the source item
        if new_from_stack.is_empty() {
            from_item.set_removed(RemovalReason::Discarded);
        } else {
            from_item.set_item(new_from_stack);
        }
    }

    /// Attempts to merge this item with nearby item entities.
    ///
    /// Mirrors vanilla's `ItemEntity.mergeWithNeighbors()`.
    /// Searches for other mergeable item entities within 0.5 blocks horizontally
    /// and attempts to merge with them.
    pub fn merge_with_neighbors(&self, world: &Arc<World>) {
        if !self.is_mergeable() {
            return;
        }

        // Search area: 0.5 blocks horizontal, 0 vertical (vanilla uses inflate(0.5, 0.0, 0.5))
        let search_box = self.bounding_box().inflate_xyz(0.5, 0.0, 0.5);

        // Get all entities in the search area
        for entity in world.get_entities_in_aabb(&search_box) {
            // Skip self
            if entity.id() == self.id() {
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
        let mut sync_state = self.sync_state.lock();
        let last_sent = sync_state.last_sent_velocity;

        let diff_sq = (current.x - last_sent.x).powi(2)
            + (current.y - last_sent.y).powi(2)
            + (current.z - last_sent.z).powi(2);

        // Sync if velocity changed significantly, or if it went to zero
        // (vanilla: ServerEntity.sendChanges lines 170-172)
        let should_sync = diff_sq > 1.0e-7
            || (diff_sq > 0.0 && current.x == 0.0 && current.y == 0.0 && current.z == 0.0);

        if should_sync {
            sync_state.last_sent_velocity = current;
            Some(CSetEntityMotion::new(
                self.id(),
                current.x,
                current.y,
                current.z,
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
        let current_on_ground = self.on_ground();
        let mut sync_state = self.sync_state.lock();
        let last_on_ground = sync_state.position.last_sent_on_ground();

        let position_changed = sync_state.position.position_changed(current_pos);
        let on_ground_changed = current_on_ground != last_on_ground;
        // Vanilla uses tickCount % 60 for periodic full position sync (FORCED_POS_UPDATE_PERIOD)
        let force_periodic_sync = tick_count % 60 == 0;

        // Vanilla: boolean pos = positionChanged || this.tickCount % 60 == 0;
        // We sync if position changed, on_ground changed, or periodic
        if !position_changed && !on_ground_changed && !force_periodic_sync {
            return None;
        }

        // Try delta encoding first
        let delta = sync_state.position.packed_delta(current_pos);

        // Use full sync if delta overflow or on-ground changed or periodic
        // (vanilla: ServerEntity.sendChanges line 123)
        let use_full_sync = on_ground_changed || force_periodic_sync || delta.is_none();

        if use_full_sync {
            // Full sync: client sets position directly, so store current_pos
            sync_state
                .position
                .mark_full_sent(current_pos, current_on_ground);

            let vel = self.velocity();
            // NOTE: We do NOT update last_sent_velocity here because the client
            // ignores the velocity field in CEntityPositionSync for non-authoritative
            // entities (like items). The velocity sync is handled separately by
            // check_velocity_sync() which sends CSetEntityMotion.

            let (yaw, pitch) = self.rotation();
            Some(PositionSyncPacket::Full(CEntityPositionSync {
                entity_id: self.id(),
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
            let (dx, dy, dz) = delta?;

            sync_state
                .position
                .mark_delta_sent(current_pos, current_on_ground);

            Some(PositionSyncPacket::Delta(CMoveEntityPos {
                entity_id: self.id(),
                dx,
                dy,
                dz,
                on_ground: current_on_ground,
            }))
        }
    }

    /// Checks blocks overlapping this item entity and calls `entity_inside`
    /// on each block's behavior (e.g. cactus destroys items).
    fn check_inside_blocks(&self) {
        use crate::behavior::BLOCK_BEHAVIORS;
        use steel_registry::blocks::block_state_ext::BlockStateExt;

        let Some(world) = self.level() else {
            return;
        };

        let aabb = self.bounding_box().deflate(1.0E-5);

        let min_x = aabb.min_x().floor() as i32;
        let min_y = aabb.min_y().floor() as i32;
        let min_z = aabb.min_z().floor() as i32;
        let max_x = aabb.max_x().floor() as i32;
        let max_y = aabb.max_y().floor() as i32;
        let max_z = aabb.max_z().floor() as i32;

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    let pos = steel_utils::BlockPos::new(x, y, z);
                    let state = world.get_block_state(pos);
                    if state.is_air() {
                        continue;
                    }
                    let block = state.get_block();
                    let behavior = BLOCK_BEHAVIORS.get_behavior(block);
                    behavior.entity_inside(state, &world, pos, self);
                    if self.is_removed() {
                        return;
                    }
                }
            }
        }
    }

    fn apply_fluid_movement_or_gravity(&self) {
        let Some(world) = self.level() else {
            self.apply_gravity();
            return;
        };

        let contact = EntityFluidContact::scan(&world, self.bounding_box());
        if contact.water_height() > ITEM_FLUID_HEIGHT_THRESHOLD {
            self.apply_fluid_movement(ITEM_WATER_DRAG);
        } else if contact.lava_height() > ITEM_FLUID_HEIGHT_THRESHOLD {
            self.apply_fluid_movement(ITEM_LAVA_DRAG);
        } else {
            self.apply_gravity();
        }
    }

    fn apply_fluid_movement(&self, horizontal_drag: f64) {
        let movement = self.velocity();
        self.set_velocity(DVec3::new(
            movement.x * horizontal_drag,
            movement.y
                + if movement.y < 0.06 {
                    FLUID_VERTICAL_NUDGE
                } else {
                    0.0
                },
            movement.z * horizontal_drag,
        ));
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
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn tick(&self) {
        // Vanilla: `Entity.tickCount` increments every tick regardless of item age/lifetime.
        let (tick_count, should_despawn) = {
            let mut state = self.item_state.lock();
            state.tick_count += 1;

            if state.pickup_delay > 0 && state.pickup_delay != INFINITE_PICKUP_DELAY {
                state.pickup_delay -= 1;
            }

            let should_despawn = if state.age == INFINITE_LIFETIME {
                false
            } else {
                state.age += 1;
                state.age >= LIFETIME
            };

            (state.tick_count, should_despawn)
        };

        // Check if item is empty
        if self.get_item().is_empty() {
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        if should_despawn {
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        // Vanilla item tick stores previous position before applying movement.
        self.set_old_position_to_current();
        let old_pos = self.old_position();
        // Store old movement for needsSync check (vanilla: ItemEntity.tick line 98)
        let old_movement = self.velocity();
        // Store old on_ground to detect changes (triggers immediate sync)
        let old_on_ground = self.on_ground();

        self.apply_fluid_movement_or_gravity();

        // Vanilla optimization: skip physics when at rest on ground.
        // Only process physics if:
        // 1. Not on ground, OR
        // 2. Has significant horizontal movement, OR
        // 3. Every 4th tick (for items that might need to fall through opened trapdoors, etc.)
        // (vanilla: ItemEntity.tick line 121)
        let vel = self.velocity();
        let horizontal_movement_sq = vel.x * vel.x + vel.z * vel.z;
        let should_move = !self.on_ground()
            || horizontal_movement_sq > 1.0e-5
            || (tick_count + self.id()) % 4 == 0;

        if should_move {
            // Move with collision detection; movement handles velocity zeroing on collision.
            if let Some(result) = self.move_entity(MoverType::SelfMovement, self.velocity()) {
                // Get world for block queries
                if let Some(world) = self.level() {
                    // Apply friction (vanilla: ItemEntity.tick line 125-128)
                    let friction = if result.on_ground
                        && let Some(block_pos) = self.block_pos_below_that_affects_movement()
                    {
                        let block_state = world.get_block_state(block_pos);
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

        // Check blocks the item overlaps (cactus destroys items, etc.)
        self.check_inside_blocks();
        if self.is_removed() {
            return;
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
            self.merge_with_neighbors(&world);
        }

        // Check if velocity changed significantly -> set needsSync (vanilla: ItemEntity.tick lines 160-164)
        // Vanilla: if (getDeltaMovement().subtract(oldMovement).lengthSqr() > 0.01) needsSync = true
        let new_movement = self.velocity();
        let diff = DVec3::new(
            new_movement.x - old_movement.x,
            new_movement.y - old_movement.y,
            new_movement.z - old_movement.z,
        );
        let diff_sq = diff.x * diff.x + diff.y * diff.y + diff.z * diff.z;
        if diff_sq > 0.01 {
            self.sync_state.lock().needs_sync = true;
        }

        // Also set needsSync when on_ground changes - this ensures immediate sync
        // when the item lands or becomes airborne, preventing client desync
        if self.on_ground() != old_on_ground {
            self.sync_state.lock().needs_sync = true;
        }
    }

    fn send_changes(&self, tick_count: i32) {
        let Some(world) = self.level() else {
            return;
        };

        let update_interval = self.entity_type().update_interval; // 20 for items
        let needs_sync = self.sync_state.lock().needs_sync;

        // Only send updates on the update interval OR when needsSync is set
        // (vanilla: ServerEntity.sendChanges line 97)
        if tick_count % update_interval != 0 && !needs_sync {
            return;
        }

        let current_pos = self.position();

        // Determine chunk for broadcasting
        let chunk_pos = steel_utils::ChunkPos::from_entity_pos(current_pos);

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
        self.sync_state.lock().needs_sync = false;
    }

    fn get_default_gravity(&self) -> f64 {
        DEFAULT_GRAVITY
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn block_pos_below_that_affects_movement(&self) -> Option<BlockPos> {
        self.on_pos(0.999_999)
    }

    fn as_item_entity(self: Arc<Self>) -> Option<Arc<ItemEntity>> {
        Some(self)
    }

    fn hurt(&self, _source: &DamageSource, amount: f32) -> bool {
        // TODO: Check isInvulnerableToBase and canBeHurtBy (damage resistance component)
        let new_health = {
            let mut state = self.item_state.lock();
            state.health -= amount as i32;
            state.health
        };
        if new_health <= 0 {
            // TODO: Call item.onDestroyed() when implemented
            self.set_removed(RemovalReason::Killed);
        }
        true
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        // Match vanilla's ItemEntity.addAdditionalSaveData
        let state = self.item_state.lock();
        nbt.insert("Health", state.health as i16);
        nbt.insert("Age", state.age as i16);
        nbt.insert("PickupDelay", state.pickup_delay as i16);

        if let Some(thrower) = state.thrower {
            nbt.insert("Thrower", NbtTag::IntArray(thrower.to_int_array().to_vec()));
        }
        if let Some(owner) = state.owner {
            nbt.insert("Owner", NbtTag::IntArray(owner.to_int_array().to_vec()));
        }
        drop(state);

        let item = self.get_item();
        if !item.is_empty() {
            nbt.insert("Item", item.to_nbt_tag());
        }
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        // Convert to view type to access accessor methods
        let nbt: NbtCompoundView<'_, '_> = nbt.into();

        // Match vanilla's ItemEntity.readAdditionalSaveData
        let mut state = self.item_state.lock();
        if let Some(health) = nbt.short("Health") {
            state.health = i32::from(health);
        }
        if let Some(age) = nbt.short("Age") {
            state.age = i32::from(age);
        }
        if let Some(pickup_delay) = nbt.short("PickupDelay") {
            state.pickup_delay = i32::from(pickup_delay);
        }

        if let Some(thrower_arr) = nbt.int_array("Thrower")
            && let Some(uuid) = Uuid::from_int_array(&thrower_arr)
        {
            state.thrower = Some(uuid);
        }
        if let Some(owner_arr) = nbt.int_array("Owner")
            && let Some(uuid) = Uuid::from_int_array(&owner_arr)
        {
            state.owner = Some(uuid);
        }
        drop(state);

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
