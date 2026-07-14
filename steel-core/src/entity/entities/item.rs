//! Item entity implementation (dropped items).
//!
//! `ItemEntity` represents a dropped item in the world. It has physics
//! (gravity, friction), despawns after 5 minutes, and can be picked up
//! by players after a short delay.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_damage_types;
use steel_registry::vanilla_entity_data::ItemEntityData;
use steel_utils::UuidExt;
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};
use uuid::Uuid;

use crate::entity::damage::DamageSource;

use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityBaseState, EntityCapabilities, EntitySyncedData,
    ItemMergeEntity, RemovalReason,
};
use crate::inventory::container::Container;
use crate::physics::MoverType;
use crate::player::Player;
use crate::world::World;

use simdnbt::ToNbtTag;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_protocol::packets::game::CTakeItemEntity;
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
const MERGE_MAX_STACK_SIZE: i32 = 64;

/// Mutable item-specific state that changes during item ticks, pickup, damage,
/// merging, and save/load.
struct ItemEntityState {
    /// Age in ticks. Despawns at `LIFETIME` (6000). Special value -32768 = infinite.
    age: i32,
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
            pickup_delay: 0,
            health: DEFAULT_HEALTH,
            thrower: None,
            owner: None,
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
#[entity_behavior]
pub struct ItemEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,

    /// Vanilla entity type registered for this implementation.
    entity_type: EntityTypeRef,

    /// Entity data containing the `ItemStack`.
    entity_data: SyncMutex<ItemEntityData>,

    /// Item-specific mutable state.
    item_state: SyncMutex<ItemEntityState>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ItemEntity`.
unsafe impl DowncastType for ItemEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/item");
}

impl ItemEntity {
    /// Creates a new item entity with an empty item.
    ///
    /// Use `set_item()` to set the actual item after creation, or use `with_item()`.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::with_item_and_velocity(
            entity_type,
            id,
            position,
            ItemStack::empty(),
            DVec3::ZERO,
            world,
        )
    }

    /// Creates a new item entity with the specified item.
    #[must_use]
    pub fn with_item(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        item: ItemStack,
        world: Weak<World>,
    ) -> Self {
        Self::with_item_and_velocity(
            entity_type,
            id,
            position,
            item,
            Self::default_spawn_velocity(),
            world,
        )
    }

    /// Creates a new item entity with the specified item and initial velocity.
    ///
    /// Mirrors vanilla's `ItemEntity(Level, double, double, double, ItemStack, double, double, double)`.
    #[must_use]
    pub fn with_item_and_velocity(
        entity_type: EntityTypeRef,
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
                EntityBaseState::new(position, entity_type.dimensions)
                    .with_velocity(velocity)
                    .with_rotation((yaw, 0.0)),
                world,
            ),
            entity_type,
            entity_data: SyncMutex::new(entity_data),
            item_state: SyncMutex::new(ItemEntityState::new()),
        }
    }

    fn default_spawn_velocity() -> DVec3 {
        DVec3::new(
            rand::random::<f64>() * 0.2 - 0.1,
            0.2,
            rand::random::<f64>() * 0.2 - 0.1,
        )
    }

    /// Creates an item entity from saved data with restored base state.
    ///
    /// Used when loading entities from disk. Type-specific data (item, age, etc.)
    /// is restored via `load_additional()` after this constructor.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(ItemEntityData::new()),
            item_state: SyncMutex::new(ItemEntityState::new()),
        }
    }

    /// Gets a clone of the item stack.
    #[must_use]
    pub fn get_item(&self) -> ItemStack {
        self.entity_data.lock().item.get().clone()
    }

    /// Sets the item stack.
    pub fn set_item(&self, item: ItemStack) {
        self.entity_data.lock().item.set(item);
    }

    /// Gets the current age in ticks.
    #[must_use]
    pub fn get_age(&self) -> i32 {
        self.item_state.lock().age
    }

    /// Sets the age in ticks.
    pub fn set_age(&self, age: i32) {
        self.item_state.lock().age = age;
    }

    /// Sets the entity to never despawn.
    pub fn set_unlimited_lifetime(&self) {
        self.item_state.lock().age = INFINITE_LIFETIME;
    }

    /// Makes this a one-tick visual pickup item that cannot be collected.
    pub fn make_fake_item(&self) {
        let mut state = self.item_state.lock();
        state.pickup_delay = INFINITE_PICKUP_DELAY;
        state.age = LIFETIME - 1;
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

    /// Gets the health (damage resistance).
    #[must_use]
    pub fn get_health(&self) -> i32 {
        self.item_state.lock().health
    }

    /// Sets the health.
    pub fn set_health(&self, health: i32) {
        self.item_state.lock().health = health;
    }

    /// Sets the entity that threw/dropped this item.
    pub fn set_thrower(&self, uuid: Uuid) {
        self.item_state.lock().thrower = Some(uuid);
    }

    /// Gets the UUID of the entity that threw/dropped this item.
    #[must_use]
    pub fn get_thrower(&self) -> Option<Uuid> {
        self.item_state.lock().thrower
    }

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
    fn try_to_merge(&self, other: &dyn ItemMergeEntity) {
        let this_stack = self.get_item();
        let other_stack = other.item_merge_stack();

        // Both items must have the same owner (target)
        if self.get_owner() != other.item_merge_owner() {
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
        to_item: &dyn ItemMergeEntity,
        to_stack: &ItemStack,
        from_item: &dyn ItemMergeEntity,
        from_stack: &ItemStack,
    ) {
        // Calculate how many items to transfer
        let max_count = to_stack.max_stack_size().min(MERGE_MAX_STACK_SIZE);
        if to_stack.count() >= max_count {
            return;
        }
        let space_available = max_count - to_stack.count();
        let transfer_count = space_available.min(from_stack.count());

        // Create new stacks
        let new_to_stack = to_stack.copy_with_count(to_stack.count() + transfer_count);
        let mut new_from_stack = from_stack.clone();
        new_from_stack.shrink(transfer_count);

        // Update the destination item
        let (from_pickup_delay, from_age) = from_item.item_merge_timing();
        to_item.apply_item_merge_destination(new_to_stack, from_pickup_delay, from_age);

        from_item.apply_item_merge_source(new_from_stack);
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
            if let Some(other_item) = entity.as_item_merge_entity() {
                // Double-check mergability (might have changed)
                if other_item.is_mergeable_item_entity() {
                    self.try_to_merge(other_item);

                    // If we've been removed (merged into other), stop
                    if self.is_removed() {
                        break;
                    }
                }
            }
        }
    }

    fn apply_fluid_movement_or_gravity(&self) {
        let contact = self.fluid_contact();
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

impl Entity for ItemEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(&self) {
        // Check if item is empty
        if self.get_item().is_empty() {
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        self.default_tick();

        {
            let mut state = self.item_state.lock();
            if state.pickup_delay > 0 && state.pickup_delay != INFINITE_PICKUP_DELAY {
                state.pickup_delay -= 1;
            }
        }

        // Vanilla item tick stores previous position before applying movement.
        self.set_old_position_to_current();
        let old_pos = self.old_position();
        // Store old movement for needsSync check (vanilla: ItemEntity.tick line 98)
        let old_movement = self.velocity();
        // Store old on_ground to detect changes (triggers immediate sync)
        let old_on_ground = self.on_ground();

        self.apply_fluid_movement_or_gravity();
        self.update_no_physics_from_current_collision();

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
            || (self.tick_count() + self.id()) % 4 == 0;

        if should_move {
            // Move with collision detection; movement handles velocity zeroing on collision.
            if let Some(result) = self.move_entity(MoverType::SelfMovement, self.velocity()) {
                self.apply_effects_from_blocks();
                if self.is_removed() {
                    return;
                }

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
        } else {
            self.apply_effects_from_blocks_for_last_movements();
            if self.is_removed() {
                return;
            }
        }

        // Item merging (vanilla: ItemEntity.tick lines 152-156)
        // Merge rate depends on whether the item moved to a different block
        let current_pos = self.position();
        let moved_block = old_pos.x.floor() as i32 != current_pos.x.floor() as i32
            || old_pos.y.floor() as i32 != current_pos.y.floor() as i32
            || old_pos.z.floor() as i32 != current_pos.z.floor() as i32;
        let merge_rate = if moved_block { 2 } else { 40 };

        if self.tick_count() % merge_rate == 0
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
            self.mark_velocity_sync();
        }

        // Also set needsSync when on_ground changes - this ensures immediate sync
        // when the item lands or becomes airborne, preventing client desync
        if self.on_ground() != old_on_ground {
            self.mark_velocity_sync();
        }

        let should_despawn = {
            let mut state = self.item_state.lock();
            if state.age == INFINITE_LIFETIME {
                false
            } else {
                state.age += 1;
                state.age >= LIFETIME
            }
        };

        if should_despawn {
            self.set_removed(RemovalReason::Discarded);
        }
    }

    fn get_default_gravity(&self) -> f64 {
        DEFAULT_GRAVITY
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn capabilities(&self) -> EntityCapabilities<'_> {
        EntityCapabilities::none().with_item_merge_entity(self)
    }

    fn block_pos_below_that_affects_movement(&self) -> Option<BlockPos> {
        self.on_pos(0.999_999)
    }

    fn attackable(&self) -> bool {
        false
    }

    fn should_play_lava_hurt_sound(&self) -> bool {
        self.get_health() <= 0 || self.tick_count() % 10 == 0
    }

    fn fire_immune(&self) -> bool {
        !self
            .get_item()
            .can_be_hurt_by(&vanilla_damage_types::IN_FIRE)
            || self.entity_type().fire_immune
    }

    fn player_touch(self: Arc<Self>, player: &Arc<Player>) {
        self.try_pickup(player);
    }

    fn hurt(&self, _world: &World, source: &DamageSource, amount: f32) -> bool {
        // TODO: Check isInvulnerableToBase once the shared non-living entity hook is ported.
        if !self.get_item().can_be_hurt_by(source.damage_type) {
            return false;
        }
        let new_health = {
            let mut state = self.item_state.lock();
            state.health = (state.health as f32 - amount) as i32;
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

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
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

impl ItemMergeEntity for ItemEntity {
    fn is_mergeable_item_entity(&self) -> bool {
        self.is_mergeable()
    }

    fn try_merge_item_entity(&self, other: &dyn ItemMergeEntity) {
        self.try_to_merge(other);
    }

    fn item_merge_stack(&self) -> ItemStack {
        self.get_item()
    }

    fn item_merge_owner(&self) -> Option<Uuid> {
        self.get_owner()
    }

    fn item_merge_timing(&self) -> (i32, i32) {
        let state = self.item_state.lock();
        (state.pickup_delay, state.age)
    }

    fn apply_item_merge_destination(&self, stack: ItemStack, pickup_delay: i32, age: i32) {
        self.set_item(stack);

        // Pickup delay is the max of both so merged items do not become instantly pickable.
        // Age is the min of both so merged items do not despawn prematurely.
        let mut state = self.item_state.lock();
        state.pickup_delay = state.pickup_delay.max(pickup_delay);
        state.age = state.age.min(age);
    }

    fn apply_item_merge_source(&self, stack: ItemStack) {
        if stack.is_empty() {
            self.set_removed(RemovalReason::Discarded);
        } else {
            self.set_item(stack);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;

    use steel_registry::{
        item_stack::ItemStack, test_support::init_test_registry, vanilla_damage_types,
        vanilla_entities, vanilla_items,
    };

    use crate::entity::{Entity, ItemMergeEntity, damage::DamageSource};
    use crate::test_support::test_world;
    use crate::world::World;

    use super::ItemEntity;

    #[test]
    fn item_entities_do_not_obstruct_block_placement() {
        let item = ItemEntity::new(
            &vanilla_entities::ITEM,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );

        assert!(!item.blocks_building());
    }

    #[test]
    fn item_lava_hurt_sound_uses_vanilla_interval() {
        let item = ItemEntity::new(
            &vanilla_entities::ITEM,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );

        assert!(item.should_play_lava_hurt_sound());
        item.advance_tick_count();
        assert!(!item.should_play_lava_hurt_sound());

        for _ in 1..10 {
            item.advance_tick_count();
        }
        assert!(item.should_play_lava_hurt_sound());

        item.set_health(0);
        item.advance_tick_count();
        assert!(item.should_play_lava_hurt_sound());
    }

    #[test]
    fn item_with_stack_uses_vanilla_default_velocity() {
        let item = ItemEntity::with_item(
            &vanilla_entities::ITEM,
            1,
            DVec3::ZERO,
            ItemStack::new(&vanilla_items::STONE),
            Weak::<World>::new(),
        );
        let velocity = item.velocity();

        assert!(velocity.x >= -0.1);
        assert!(velocity.x < 0.1);
        assert_eq!(velocity.y.to_bits(), 0.2_f64.to_bits());
        assert!(velocity.z >= -0.1);
        assert!(velocity.z < 0.1);
    }

    #[test]
    fn fake_item_is_never_pickable_and_expires_on_its_next_tick() {
        let item = ItemEntity::with_item(
            &vanilla_entities::ITEM,
            1,
            DVec3::ZERO,
            ItemStack::new(&vanilla_items::STONE),
            Weak::<World>::new(),
        );

        item.make_fake_item();

        assert_eq!(item.get_pickup_delay(), 32_767);
        assert_eq!(item.get_age(), 5_999);
    }

    #[test]
    fn item_merge_capability_preserves_vanilla_stack_and_timing() {
        init_test_registry();

        let source = ItemEntity::with_item(
            &vanilla_entities::ITEM,
            1,
            DVec3::ZERO,
            ItemStack::with_count(&vanilla_items::STONE, 10),
            Weak::<World>::new(),
        );
        source.set_pickup_delay(5);
        source.set_age(20);

        let target = ItemEntity::with_item(
            &vanilla_entities::ITEM,
            2,
            DVec3::ZERO,
            ItemStack::with_count(&vanilla_items::STONE, 20),
            Weak::<World>::new(),
        );
        target.set_pickup_delay(1);
        target.set_age(50);

        assert!(source.as_item_merge_entity().is_some());
        source.try_merge_item_entity(&target);

        assert!(source.is_removed());
        assert_eq!(target.get_item().count(), 30);
        assert_eq!(target.get_pickup_delay(), 5);
        assert_eq!(target.get_age(), 20);
    }

    #[test]
    fn item_damage_truncates_after_fractional_subtraction() {
        let item = ItemEntity::new(
            &vanilla_entities::ITEM,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );

        assert!(item.hurt(
            test_world(),
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            0.75
        ));

        assert_eq!(item.get_health(), 4);
    }

    #[test]
    fn damage_resistant_item_ignores_matching_damage() {
        init_test_registry();

        let item = ItemEntity::with_item(
            &vanilla_entities::ITEM,
            1,
            DVec3::ZERO,
            ItemStack::new(&vanilla_items::NETHERITE_INGOT),
            Weak::<World>::new(),
        );

        assert!(item.fire_immune());
        assert!(!item.hurt(
            test_world(),
            &DamageSource::environment(&vanilla_damage_types::IN_FIRE),
            1.0
        ));
        assert_eq!(item.get_health(), 5);

        assert!(item.hurt(
            test_world(),
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            1.0
        ));
        assert_eq!(item.get_health(), 4);
    }
}
