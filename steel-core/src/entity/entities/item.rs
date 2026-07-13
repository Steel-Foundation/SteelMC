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
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::ItemEntityData;
use steel_utils::UuidExt;
use steel_utils::{DowncastType, DowncastTypeKey};
use uuid::Uuid;

use crate::entity::damage::DamageSource;

use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityBaseState, EntitySyncedData, RemovalReason,
    SharedEntity,
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

/// A dropped item entity.
///
/// Mirrors vanilla's `ItemEntity` behavior:
/// - Falls with gravity (0.04 per tick)
/// - Applies friction when on ground (0.98)
/// - Despawns after 5 minutes (6000 ticks)
/// - Has pickup delay before players can collect it
#[entity_behavior(class = "item_entity", identifier = "item")]
pub struct ItemEntity {
    /// Weak back-reference to the containing `EntityBase`.
    base: Weak<EntityBase>,
    /// Entity data containing the `ItemStack`.
    entity_data: ItemEntityData,

    /// Age in ticks. Despawns at `LIFETIME` (6000). Special value -32768 = infinite.
    age: i32,
    /// Ticks until pickupable. 0 = can pickup, 32767 = never.
    pickup_delay: i32,
    /// Health (damage resistance). Item is destroyed when this reaches 0.
    health: i32,
    /// UUID of the entity that threw/dropped this item.
    pub thrower: Option<Uuid>,
    /// UUID of the only entity that can pick up this item.
    /// If `None`, any player can pick it up. Vanilla calls this `target`.
    owner: Option<Uuid>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ItemEntity`.
unsafe impl DowncastType for ItemEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/item");
}

impl ItemEntity {
    /// Creates a new item entity with an empty item. Returns a full `SharedEntity`.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>) -> SharedEntity {
        Self::with_item_and_velocity(id, position, ItemStack::empty(), DVec3::ZERO, world)
    }

    /// Creates an item entity `SharedEntity` from saved data.
    ///
    /// Type-specific data (item, age, etc.) is restored via `load_additional()`
    /// after this constructor.
    #[must_use]
    pub fn from_saved(load: EntityBaseLoad) -> SharedEntity {
        EntityBase::pack_loaded_with(load, vanilla_entities::ITEM.dimensions, |base| {
            Self::from_weak_base(base)
        })
    }

    #[must_use]
    fn from_weak_base(base: Weak<EntityBase>) -> Self {
        Self {
            base,
            entity_data: ItemEntityData::new(),
            age: 0,
            pickup_delay: 0,
            health: DEFAULT_HEALTH,
            thrower: None,
            owner: None,
        }
    }

    /// Creates a new item entity with an empty item. Returns a full `SharedEntity`.
    #[must_use]
    pub fn create(id: i32, position: DVec3, world: Weak<World>) -> SharedEntity {
        Self::with_item_and_velocity(id, position, ItemStack::empty(), DVec3::ZERO, world)
    }

    /// Creates a new item entity with the specified item.
    #[must_use]
    pub fn with_item(
        id: i32,
        position: DVec3,
        item: ItemStack,
        world: Weak<World>,
    ) -> SharedEntity {
        Self::with_item_and_velocity(id, position, item, Self::default_spawn_velocity(), world)
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
    ) -> SharedEntity {
        let yaw = rand::random::<f32>() * 360.0;
        Arc::new_cyclic(|weak: &Weak<EntityBase>| {
            let mut entity_data = ItemEntityData::new();
            entity_data.set_item(item);
            let inner = Self {
                base: weak.clone(),
                entity_data: entity_data,
                age: 0,
                pickup_delay: 0,
                health: DEFAULT_HEALTH,
                thrower: None,
                owner: None,
            };
            let mut base = EntityBase::new_with_state(
                id,
                EntityBaseState::new(position, vanilla_entities::ITEM.dimensions)
                    .with_velocity(velocity)
                    .with_rotation((yaw, 0.0)),
                world,
            );
            base.attach_entity(inner);
            base
        })
    }

    fn default_spawn_velocity() -> DVec3 {
        DVec3::new(
            rand::random::<f64>() * 0.2 - 0.1,
            0.2,
            rand::random::<f64>() * 0.2 - 0.1,
        )
    }

    /// Gets a clone of the item stack.
    #[must_use]
    pub fn get_item(&self) -> ItemStack {
        self.entity_data.item.get().clone()
    }

    /// Sets the item stack.
    pub fn set_item(&mut self, item: ItemStack) {
        self.entity_data.set_item(item);
    }

    /// Sets the default pickup delay (10 ticks = 0.5 seconds).
    pub fn set_default_pickup_delay(&mut self) {
        self.pickup_delay = DEFAULT_PICKUP_DELAY;
    }

    /// Sets a custom pickup delay in ticks.
    pub fn set_pickup_delay(&mut self, delay: i32) {
        self.pickup_delay = delay;
    }

    /// Attempts to have a player pick up this item.
    ///
    /// Returns `true` if the item was fully picked up (and the entity should be removed),
    /// `false` if pickup failed or was only partial.
    ///
    /// Mirrors vanilla's `ItemEntity.playerTouch(Player)`.
    pub fn try_pickup(&mut self, player: &mut Player) -> bool {
        // Check pickup delay
        if self.pickup_delay > 0 {
            return false;
        }

        // Check owner restriction
        if let Some(owner_uuid) = self.owner
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
        !self.is_removed()
            && self.pickup_delay != INFINITE_PICKUP_DELAY
            && self.age != INFINITE_LIFETIME
            && self.age < LIFETIME
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
    fn try_to_merge(&mut self, other: &mut ItemEntity) {
        let this_stack = self.get_item();
        let other_stack = other.get_item();

        // Both items must have the same owner (target)
        if self.owner != other.owner {
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
        to_item: &mut ItemEntity,
        to_stack: &ItemStack,
        from_item: &mut ItemEntity,
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
        to_item.set_item(new_to_stack);

        // Pickup delay is the max of both (so merged items don't become instantly pickable)
        // Age is the min of both (so merged items don't despawn prematurely).
        let (from_pickup_delay, from_age) = { (from_item.pickup_delay, from_item.age) };
        {
            to_item.pickup_delay = to_item.pickup_delay.max(from_pickup_delay);
            to_item.age = to_item.age.min(from_age);
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
    pub fn merge_with_neighbors(&mut self, world: &Arc<World>) {
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
            // Filter by type lock-free before locking: `with_entity_as` locks the
            // entity, so never lock a non-item (which may already be locked
            // elsewhere). Only items merge with items.
            if entity.entity_type() != &vanilla_entities::ITEM {
                continue;
            }

            entity.with_entity_as::<Self, _>(|other_item| {
                // Double-check mergability (might have changed)
                if other_item.is_mergeable() {
                    self.try_to_merge(other_item);
                }
            });
            // If we've been removed (merged into other), stop
            if self.is_removed() {
                break;
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
    fn base_weak(&self) -> &Weak<EntityBase> {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn tick(&mut self) {
        // Check if item is empty
        if self.get_item().is_empty() {
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        self.default_tick();

        {
            if self.pickup_delay > 0 && self.pickup_delay != INFINITE_PICKUP_DELAY {
                self.pickup_delay -= 1;
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
            if self.age == INFINITE_LIFETIME {
                false
            } else {
                self.age += 1;
                self.age >= LIFETIME
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

    fn synced_data_mut(&mut self) -> Option<&mut dyn EntitySyncedData> {
        Some(&mut self.entity_data)
    }

    fn block_pos_below_that_affects_movement(&self) -> Option<BlockPos> {
        self.on_pos(0.999_999)
    }

    fn attackable(&self) -> bool {
        false
    }

    fn should_play_lava_hurt_sound(&self) -> bool {
        self.health <= 0 || self.tick_count() % 10 == 0
    }

    fn player_touch(&mut self, player: &mut Player) {
        self.try_pickup(player);
    }

    fn hurt(&mut self, _source: &DamageSource, amount: f32) -> bool {
        // TODO: Check isInvulnerableToBase and canBeHurtBy (damage resistance component)
        let new_health = {
            self.health = (self.health as f32 - amount) as i32;
            self.health
        };
        if new_health <= 0 {
            // TODO: Call item.onDestroyed() when implemented
            self.set_removed(RemovalReason::Killed);
        }
        true
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        // Match vanilla's ItemEntity.addAdditionalSaveData
        nbt.insert("Health", self.health as i16);
        nbt.insert("Age", self.age as i16);
        nbt.insert("PickupDelay", self.pickup_delay as i16);

        if let Some(thrower) = self.thrower {
            nbt.insert("Thrower", NbtTag::IntArray(thrower.to_int_array().to_vec()));
        }
        if let Some(owner) = self.owner {
            nbt.insert("Owner", NbtTag::IntArray(owner.to_int_array().to_vec()));
        }

        let item = self.get_item();
        if !item.is_empty() {
            nbt.insert("Item", item.to_nbt_tag());
        }
    }

    fn load_additional(&mut self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        // Match vanilla's ItemEntity.readAdditionalSaveData
        if let Some(health) = nbt.short("Health") {
            self.health = i32::from(health);
        }
        if let Some(age) = nbt.short("Age") {
            self.age = i32::from(age);
        }
        if let Some(pickup_delay) = nbt.short("PickupDelay") {
            self.pickup_delay = i32::from(pickup_delay);
        }

        if let Some(thrower_arr) = nbt.int_array("Thrower")
            && let Some(uuid) = Uuid::from_int_array(&thrower_arr)
        {
            self.thrower = Some(uuid);
        }
        if let Some(owner_arr) = nbt.int_array("Owner")
            && let Some(uuid) = Uuid::from_int_array(&owner_arr)
        {
            self.owner = Some(uuid);
        }

        if let Some(item_tag) = nbt.compound("Item")
            && let Some(item) = ItemStack::from_borrowed_compound(&item_tag)
        {
            self.entity_data.set_item(item);
        }

        // Vanilla behavior: discard if item is empty after load
        if self.get_item().is_empty() {
            self.set_removed(RemovalReason::Discarded);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;

    use steel_registry::{item_stack::ItemStack, vanilla_damage_types, vanilla_items};

    use crate::entity::{Entity, damage::DamageSource};

    use super::ItemEntity;

    #[test]
    fn item_entities_do_not_obstruct_block_placement() {
        let item = ItemEntity::create(1, DVec3::ZERO, Weak::new());

        assert!(!item.with_entity(|e| e.blocks_building()));
    }

    #[test]
    fn item_lava_hurt_sound_uses_vanilla_interval() {
        let item = ItemEntity::create(1, DVec3::ZERO, Weak::new());

        {
            let mut item = item.lock_entity();
            let item: &mut ItemEntity = item.downcast().unwrap();

            assert!(item.should_play_lava_hurt_sound());
            item.advance_tick_count();
            assert!(!item.should_play_lava_hurt_sound());

            for _ in 1..10 {
                item.advance_tick_count();
            }
            assert!(item.should_play_lava_hurt_sound());

            item.health = 0;
            item.advance_tick_count();
            assert!(item.should_play_lava_hurt_sound());
        }
    }

    #[test]
    fn item_with_stack_uses_vanilla_default_velocity() {
        let item = ItemEntity::with_item(
            1,
            DVec3::ZERO,
            ItemStack::new(&vanilla_items::ITEMS.stone),
            Weak::new(),
        );
        let velocity = item.velocity();

        assert!(velocity.x >= -0.1);
        assert!(velocity.x < 0.1);
        assert_eq!(velocity.y.to_bits(), 0.2_f64.to_bits());
        assert!(velocity.z >= -0.1);
        assert!(velocity.z < 0.1);
    }

    #[test]
    fn item_damage_truncates_after_fractional_subtraction() {
        let item = ItemEntity::create(1, DVec3::ZERO, Weak::new());

        assert!(item.with_entity(|e| e.hurt(
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            0.75
        )));

        let mut item = item.lock_entity();
        let item: &mut ItemEntity = item.downcast().unwrap();

        assert_eq!(item.health, 4);
    }
}
