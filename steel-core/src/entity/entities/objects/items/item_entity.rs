//! Item entity implementation (dropped items).
//!
//! `ItemEntity` represents a dropped item in the world. It has physics
//! (gravity, friction), despawns after 5 minutes, and can be picked up
//! by players after a short delay.

use crate::entity::component::definition::Definition;
use crate::entity::component::definition::HurtCtx;
use crate::entity::component::pipeline::{Flow, StageId};
use crate::entity::component::pipeline::{Pipeline, Stage};
use crate::entity::prelude::*;
use std::sync::{Arc, Weak};
use steel_utils::nbt::NbtValueInput as _;

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_math::DEGREE_360;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::ItemEntityData;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_registry::{vanilla_damage_types, vanilla_game_events};
use steel_utils::Downcast as _;
use steel_utils::UuidExt;
use steel_utils::locks::CheckedMutex;
use uuid::Uuid;

use crate::behavior::ITEM_BEHAVIORS;
use crate::entity::SharedEntity;
use crate::entity::damage::DamageSource;
use crate::entity::{
    DefaultEntityPush, Entity, EntityBase, EntityBaseLoad, EntityBaseState, EntityMovementEmission,
    RemovalReason,
};
use crate::inventory::container::Container;
use crate::physics::MoverType;
use crate::player::Player;
use crate::world::World;
use text_components::TextComponent;

use crate::entity::EntityProps;
use simdnbt::ToNbtTag;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_protocol::packets::game::CTakeItemEntity;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::stat::vanilla_stat_types;

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

const FLOAT_HEIGHT: f32 = 0.1;

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
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_item, load = Self::load_item)]
#[entity(key = "steel:entity/item", definition = Self::define)]
pub struct ItemEntity {
    /// Common entity fields (id, uuid, position, etc.).
    #[entity(base)]
    base: EntityBase,

    /// Vanilla entity type registered for this implementation.
    #[entity(entity_type)]
    entity_type: EntityTypeRef,

    /// Entity data containing the `ItemStack`.
    #[entity(synced)]
    entity_data: CheckedMutex<ItemEntityData>,

    /// Item-specific mutable state.
    state: CheckedMutex<ItemEntityState>,
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
        let yaw = rand::random_range(0.0..DEGREE_360);

        let mut entity = Self::from_base(
            EntityBase::new_with_state(
                id,
                EntityBaseState::new(position, entity_type.dimensions)
                    .with_velocity(velocity)
                    .with_rotation((yaw, 0.0)),
                world,
            ),
            entity_type,
        );
        entity.entity_data.get_mut().item.set(item);
        entity
    }

    pub(crate) fn default_spawn_velocity() -> DVec3 {
        DVec3::new(
            rand::random_range(-0.1..0.1),
            0.2,
            rand::random_range(-0.1..0.1),
        )
    }

    /// Creates an item entity from saved data with restored base state.
    ///
    /// Used when loading entities from disk. Type-specific data (item, age, etc.)
    /// is restored via `load_additional()` after this constructor.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        Self {
            base,
            entity_type,
            entity_data: CheckedMutex::new(ItemEntityData::new()),
            state: CheckedMutex::new(ItemEntityState::new()),
        }
    }

    /// Gets a clone of the item stack.
    #[must_use]
    pub fn item(&self) -> ItemStack {
        self.entity_data.lock().item.get().clone()
    }

    /// Sets the item stack.
    pub fn set_item(&self, item: ItemStack) {
        self.entity_data.lock().item.set(item);
    }

    /// Gets the current age in ticks.
    #[must_use]
    pub fn age(&self) -> i32 {
        self.state.lock().age
    }

    /// Sets the age in ticks.
    pub fn set_age(&self, age: i32) {
        self.state.lock().age = age;
    }

    /// Makes this a one-tick visual pickup item that cannot be collected.
    pub fn make_fake_item(&self) {
        let mut state = self.state.lock();
        state.pickup_delay = INFINITE_PICKUP_DELAY;
        state.age = LIFETIME - 1;
    }

    /// Gets the pickup delay in ticks.
    #[must_use]
    pub fn pickup_delay(&self) -> i32 {
        self.state.lock().pickup_delay
    }

    /// Sets the default pickup delay (10 ticks = 0.5 seconds).
    pub fn set_default_pickup_delay(&self) {
        self.state.lock().pickup_delay = DEFAULT_PICKUP_DELAY;
    }

    /// Sets the pickup delay to zero (immediately pickupable).
    pub fn set_no_pickup_delay(&self) {
        self.state.lock().pickup_delay = 0;
    }

    /// Sets a custom pickup delay in ticks.
    pub fn set_pickup_delay(&self, delay: i32) {
        self.state.lock().pickup_delay = delay;
    }

    /// Returns true if the item has a pickup delay (cannot be picked up yet).
    #[must_use]
    pub fn has_pickup_delay(&self) -> bool {
        self.state.lock().pickup_delay > 0
    }

    /// Gets the health (damage resistance).
    #[must_use]
    pub fn health(&self) -> i32 {
        self.state.lock().health
    }

    /// Sets the health.
    pub fn set_health(&self, health: i32) {
        self.state.lock().health = health;
    }

    /// Sets the entity that threw/dropped this item.
    pub fn set_thrower(&self, uuid: Uuid) {
        self.state.lock().thrower = Some(uuid);
    }

    /// Gets the UUID of the entity that threw/dropped this item.
    #[must_use]
    pub fn thrower(&self) -> Option<Uuid> {
        self.state.lock().thrower
    }

    /// Sets the owner (the only entity that can pick up this item).
    ///
    /// Pass `None` to allow any player to pick it up.
    /// Vanilla calls this `target`.
    pub fn set_owner(&self, uuid: Option<Uuid>) {
        self.state.lock().owner = uuid;
    }

    /// Gets the owner UUID (the only entity that can pick up this item).
    ///
    /// Returns `None` if any player can pick it up.
    #[must_use]
    pub fn owner(&self) -> Option<Uuid> {
        self.state.lock().owner
    }

    /// Attempts to have a player pick up this item.
    ///
    /// Returns `true` if the item was fully picked up (and the entity should be removed),
    /// `false` if pickup failed or was only partial.
    pub fn try_pickup(&self, player: &Arc<Player>) -> bool {
        // Vanilla requires exactly 0, so a negative loaded delay blocks pickup.
        if self.state.lock().pickup_delay != 0 {
            return false;
        }

        if let Some(owner_uuid) = self.owner()
            && owner_uuid != player.gameprofile.id()
        {
            return false;
        }

        let mut item = self.item();
        let original_count = item.count();
        if !player.inventory.lock().add(&mut item) {
            // What fit is moved into the inventory; the rest stays on the ground
            // without a take animation or stat.
            if item.count() != original_count {
                self.set_item(item);
            }
            return false;
        }

        if let Some(world) = self.level() {
            let chunk_pos = steel_utils::ChunkPos::from_entity_pos(self.position());
            let take_packet = CTakeItemEntity::new(self.id(), player.id(), original_count);
            world.broadcast_to_nearby(chunk_pos, take_packet, None);
        }
        self.set_removed(RemovalReason::Discarded);
        player.award_stat_with_count(
            &vanilla_stat_types::ITEM_PICKED_UP,
            item.item,
            original_count,
        );
        true
    }

    /// Returns true if this item entity can be merged with others.
    ///
    /// An item is mergeable if:
    /// - It's not removed
    /// - It doesn't have infinite pickup delay (32767)
    /// - It doesn't have infinite lifetime (-32768)
    /// - Its age is less than the despawn threshold (6000)
    /// - Its count is less than max stack size
    #[must_use]
    pub fn is_mergeable(&self) -> bool {
        let item = self.item();
        let state = self.state.lock();
        !self.is_removed()
            && state.pickup_delay != INFINITE_PICKUP_DELAY
            && state.age != INFINITE_LIFETIME
            && state.age < LIFETIME
            && item.count() < item.max_stack_size()
    }

    /// Checks if two item stacks can be merged together.
    ///
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
    /// The item with fewer items is merged into the one with more.
    fn try_to_merge(&self, other: &Self) {
        let this_stack = self.item();
        let other_stack = other.item();

        // Both items must have the same owner (target)
        if self.owner() != other.owner() {
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
    fn merge_stacks(
        to_item: &Self,
        to_stack: &ItemStack,
        from_item: &Self,
        from_stack: &ItemStack,
    ) {
        // Vanilla `ItemEntity.merge`: negative when `to_stack` is already above 64,
        // which moves items back to `from_stack`.
        let transfer_count =
            (to_stack.max_stack_size().min(64) - to_stack.count()).min(from_stack.count());

        // Create new stacks
        let new_to_stack = to_stack.copy_with_count(to_stack.count() + transfer_count);
        let mut new_from_stack = from_stack.clone();
        new_from_stack.shrink(transfer_count);

        // Update the destination item
        let (from_pickup_delay, from_age) = {
            let state = from_item.state.lock();
            (state.pickup_delay, state.age)
        };
        to_item.set_item(new_to_stack);

        // Pickup delay is the max of both so merged items do not become instantly pickable.
        // Age is the min of both so merged items do not despawn prematurely.
        {
            let mut state = to_item.state.lock();
            state.pickup_delay = state.pickup_delay.max(from_pickup_delay);
            state.age = state.age.min(from_age);
        }

        if new_from_stack.is_empty() {
            from_item.set_removed(RemovalReason::Discarded);
        } else {
            from_item.set_item(new_from_stack);
        }
    }

    /// Attempts to merge this item with nearby item entities.
    ///
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

            if let Some(other_item) = entity.downcast_ref::<Self>() {
                // Double-check mergability (might have changed)
                if other_item.is_mergeable() {
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
        if contact.water_height() > f64::from(FLOAT_HEIGHT) {
            self.apply_fluid_movement(0.99);
        } else if contact.lava_height() > f64::from(FLOAT_HEIGHT) {
            self.apply_fluid_movement(0.95);
        } else {
            self.apply_gravity();
        }
    }

    fn apply_fluid_movement(&self, multiplier: f32) {
        let movement = self.velocity();
        let multiplier = f64::from(multiplier);
        let lift = if movement.y < f64::from(0.06_f32) {
            f64::from(5.0e-4_f32)
        } else {
            0.0
        };
        self.set_velocity(DVec3::new(
            movement.x * multiplier,
            movement.y + lift,
            movement.z * multiplier,
        ));
    }
}

impl DefaultEntityPush for ItemEntity {}

impl ItemEntity {
    const TICK: StageId = StageId("item.tick");

    const HURT: StageId = StageId("item.hurt");

    fn define() -> Definition<Self> {
        let definition = Definition::new();
        Definition {
            tick: definition.tick.around(Self::TICK, |item, entity, inner| {
                let shared = entity.entity;
                item.entity_tick(shared, || {
                    inner(item, entity);
                });
                Flow::Continue
            }),
            hurt: Pipeline::new([Stage::new(
                Self::HURT,
                |item: &Self, ctx: &mut HurtCtx<'_>| {
                    ctx.result = item.entity_hurt(ctx.world, ctx.source, ctx.amount);
                    Flow::Continue
                },
            )]),
            ..definition
        }
    }

    const ENTITY: EntityProps<Self> = EntityProps {
        is_hard_collision_relevant: |_| false,
        ignore_explosion: |_, explosion| !explosion.should_affect_blocklike_entities(),
        movement_emission: |_| EntityMovementEmission::None,
        default_gravity: |_| 0.04,
        block_pos_below_that_affects_movement: |entity| entity.on_pos(0.999_999),
        attackable: |_| false,
        should_play_lava_hurt_sound: Self::entity_should_play_lava_hurt_sound,
        fire_immune: Self::entity_fire_immune,
        player_touch: Self::entity_player_touch,
        name: Self::entity_name,
        on_teleported: |item| {
            if let Some(world) = item.level() {
                item.merge_with_neighbors(&world);
            }
        },
        ..EntityProps::new()
    };

    /// Unlike other entities, the custom name keeps its click and hover actions.
    fn entity_name(&self) -> TextComponent {
        self.custom_name().unwrap_or_else(|| {
            let item = self.item();
            ITEM_BEHAVIORS
                .get_behavior(item.item())
                .get_name(&item)
                .into_owned()
        })
    }

    /// `ItemEntity.tick`: an empty stack is discarded instead of ticking.
    fn entity_tick(&self, entity: &SharedEntity, super_tick: impl FnOnce()) {
        if self.item().is_empty() {
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        super_tick();

        {
            let mut state = self.state.lock();
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
            || horizontal_movement_sq > f64::from(1.0e-5_f32)
            || (self.tick_count() + self.id()) % 4 == 0;

        if should_move {
            // Move with collision detection; movement handles velocity zeroing on collision.
            if let Some(result) =
                Arc::clone(entity).move_entity(MoverType::SelfMovement, self.velocity())
            {
                self.apply_effects_from_blocks();
                if self.is_removed() {
                    return;
                }

                // Get world for block queries
                if let Some(world) = self.level() {
                    // Apply friction (vanilla: ItemEntity.tick line 125-128)
                    let air_drag = self.air_drag();
                    let ground_friction = if result.on_ground
                        && let Some(block_pos) = self.block_pos_below_that_affects_movement()
                    {
                        air_drag * world.get_block_state(block_pos).get_block().config.friction
                    } else {
                        air_drag
                    };

                    let mut velocity = self.velocity();
                    velocity.x *= f64::from(ground_friction);
                    velocity.z *= f64::from(ground_friction);
                    velocity.y *= f64::from(air_drag);

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

        // Vanilla item ticks refresh fluid interaction after movement and sync
        // the result, which also emits water-entry splashes.
        if self.update_fluid_interaction() {
            self.mark_velocity_sync();
        }

        // Check if velocity changed significantly -> set needsSync (vanilla: ItemEntity.tick lines 160-164)
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
            let mut state = self.state.lock();
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

    fn entity_should_play_lava_hurt_sound(&self) -> bool {
        self.health() <= 0 || self.tick_count() % 10 == 0
    }

    fn entity_fire_immune(&self) -> bool {
        !self.item().can_be_hurt_by(&vanilla_damage_types::IN_FIRE)
            || self.entity_type().fire_immune
    }

    fn entity_player_touch(&self, player: &Arc<Player>) {
        self.try_pickup(player);
    }

    fn entity_hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        if self.is_invulnerable_to_base(source) {
            return false;
        }
        if !world.get_game_rule(&MOB_GRIEFING)
            && source
                .causing_entity()
                .is_some_and(|entity| entity.as_mob().is_some())
        {
            return false;
        }
        if !self.item().can_be_hurt_by(source.damage_type) {
            return false;
        }
        self.mark_hurt();
        let new_health = {
            let mut state = self.state.lock();
            state.health = (state.health as f32 - amount) as i32;
            state.health
        };
        self.game_event_with_source_entity(
            &vanilla_game_events::ENTITY_DAMAGE,
            source.causing_entity().as_ref().map(AsRef::as_ref),
        );
        if new_health <= 0 {
            // TODO: Call item.onDestroyed() when implemented
            self.set_removed(RemovalReason::Discarded);
        }
        true
    }

    fn save_item(&self, nbt: &mut NbtCompound) {
        // Match vanilla's ItemEntity.addAdditionalSaveData
        let state = self.state.lock();
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

        let item = self.item();
        if !item.is_empty() {
            nbt.insert("Item", item.to_nbt_tag());
        }
    }

    fn load_item(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        // Match vanilla's ItemEntity.readAdditionalSaveData
        let mut state = self.state.lock();
        if let Some(health) = nbt.short_value("Health") {
            state.health = i32::from(health);
        }
        if let Some(age) = nbt.short_value("Age") {
            state.age = i32::from(age);
        }
        if let Some(pickup_delay) = nbt.short_value("PickupDelay") {
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
        if self.item().is_empty() {
            self.set_removed(RemovalReason::Discarded);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::prelude::*;
    use std::sync::{Arc, Weak};

    use glam::DVec3;

    use steel_registry::{
        init_vanilla_registry, item_stack::ItemStack, vanilla_blocks, vanilla_damage_types,
        vanilla_entities, vanilla_items,
    };
    use steel_utils::{BlockPos, types::UpdateFlags};

    use steel_registry::data_components::vanilla_components::ITEM_NAME;

    use crate::behavior::init_behaviors;
    use crate::entity::{Entity, damage::DamageSource};
    use crate::test_support::{fresh_loaded_test_world, test_world};
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
    fn unnamed_item_entity_is_named_after_its_stack() {
        init_vanilla_registry();
        init_behaviors();
        let item = ItemEntity::new(
            &vanilla_entities::ITEM,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        let stack = ItemStack::new(&vanilla_items::DIAMOND);
        let Some(item_name) = stack.get(ITEM_NAME).cloned() else {
            panic!("diamond should have an item name");
        };
        item.set_item(stack);

        assert_eq!(item.name(), item_name);
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
    fn item_tick_refreshes_fluid_after_movement() {
        init_vanilla_registry();
        init_behaviors();
        let fixture = fresh_loaded_test_world("item_post_move_fluid_update");
        let world = &fixture.world;
        assert!(world.set_block(
            BlockPos::new(8, 64, 8),
            vanilla_blocks::WATER.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let item = Arc::new(ItemEntity::with_item(
            &vanilla_entities::ITEM,
            1,
            DVec3::new(8.5, 65.1, 8.5),
            ItemStack::new(&vanilla_items::STONE),
            Arc::downgrade(world),
        ));
        item.set_velocity(DVec3::new(0.0, -0.2, 0.0));

        Arc::clone(&item).tick();

        assert!(item.fluid_contact().water_height() > 0.0);
        assert!(item.needs_velocity_sync());
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

        assert_eq!(item.pickup_delay(), 32_767);
        assert_eq!(item.age(), 5_999);
    }

    #[test]
    fn item_merge_preserves_vanilla_stack_and_timing() {
        init_vanilla_registry();

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

        source.try_to_merge(&target);

        assert!(source.is_removed());
        assert_eq!(target.item().count(), 30);
        assert_eq!(target.pickup_delay(), 5);
        assert_eq!(target.age(), 20);
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

        assert_eq!(item.health(), 4);
    }

    #[test]
    fn damage_resistant_item_ignores_matching_damage() {
        init_vanilla_registry();

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
        assert_eq!(item.health(), 5);

        assert!(item.hurt(
            test_world(),
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            1.0
        ));
        assert_eq!(item.health(), 4);
    }
}
