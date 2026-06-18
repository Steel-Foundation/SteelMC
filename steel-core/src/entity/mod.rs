//! This module contains entity-related traits and types.

use std::sync::{Arc, LazyLock, Weak};

use glam::DVec3;
use rustc_hash::FxHashSet;
use simdnbt::borrow::BaseNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_protocol::packets::game::{AttributeSnapshot, CEntityEvent, SoundSource};
use steel_registry::blocks::{
    block_state_ext::BlockStateExt as _, properties::BlockStateProperties,
    shapes::is_shape_full_block,
};
use steel_registry::data_components::vanilla_components::{EquippableSlot, GLIDER};
use steel_registry::entity_data::{DataValue, EntityPose};
use steel_registry::entity_type::{EntityAttachment, EntityTypeRef};
use steel_registry::fluid::FluidState;
use steel_registry::item_stack::ItemStack;
use steel_registry::mob_effect::MobEffectRef;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_type_tags::EntityTypeTag;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, sound_events, vanilla_damage_types, vanilla_game_events,
};
use steel_registry::{vanilla_attributes, vanilla_fluid_tags, vanilla_items, vanilla_mob_effects};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::random::Random as _;
use steel_utils::{BlockPos, BlockStateId, ChunkPos, Direction, Identifier, WorldAabb, axis::Axis};
use uuid::Uuid;

use crate::behavior::{
    BLOCK_BEHAVIORS, BlockCollisionContext, BlockStateBehaviorExt as _, EntityFallOnContext,
    EntityLandingContext, FLUID_BEHAVIORS,
};
use crate::entity::attribute::AttributeMap;
use crate::fluid::{LavaFluid, get_fluid_state, get_height};
use crate::inventory::equipment::EquipmentSlot;
use crate::physics::{
    COLLISION_EPSILON, CollisionWorld, EntityPhysicsState, MoveResult, MoverType,
    WorldCollisionProvider, move_entity as resolve_entity_movement,
};
use crate::world::game_event_context::GameEventContext;
use crate::world::{ClipBlockShape, ClipFluid, World};
use crate::{entity::damage::DamageSource, player::Player};

use entities::ItemEntity;

/// Global counter for allocating unique entity IDs.
///
/// Mirrors vanilla's `Entity.ENTITY_COUNTER`. Each new entity increments this
/// counter to get a unique network ID. Starts at 1 (0 is reserved).
static ENTITY_COUNTER: LazyLock<SyncMutex<i32>> = LazyLock::new(|| SyncMutex::new(1));
const MOVEMENT_RECORD_EPSILON: f64 = 1.0e-7;
const NO_PHYSICS_COLLISION_EPSILON: f64 = 1.0e-7;
const WATER_ENTITY_FLOW_SCALE: f64 = 0.014;
const MOVE_TOWARDS_CLOSEST_SPACE_DIRECTIONS: [Direction; 5] = [
    Direction::North,
    Direction::South,
    Direction::West,
    Direction::East,
    Direction::Up,
];

fn horizontal_distance(vector: DVec3) -> f64 {
    vector.x.hypot(vector.z)
}

fn fall_flying_collision_damage(previous_horizontal_speed: f64, new_horizontal_speed: f64) -> f32 {
    ((previous_horizontal_speed - new_horizontal_speed) * 10.0 - 3.0) as f32
}

const fn fall_flying_free_fall_interval(fall_flying_ticks: i32) -> Option<i32> {
    let check_fall_flying_ticks = fall_flying_ticks.wrapping_add(1);
    if check_fall_flying_ticks % 10 == 0 {
        Some(check_fall_flying_ticks / 10)
    } else {
        None
    }
}

const fn equipment_slot_matches_equippable(
    slot: EquipmentSlot,
    equippable_slot: EquippableSlot,
) -> bool {
    matches!(
        (slot, equippable_slot),
        (EquipmentSlot::MainHand, EquippableSlot::Mainhand)
            | (EquipmentSlot::OffHand, EquippableSlot::Offhand)
            | (EquipmentSlot::Feet, EquippableSlot::Feet)
            | (EquipmentSlot::Legs, EquippableSlot::Legs)
            | (EquipmentSlot::Chest, EquippableSlot::Chest)
            | (EquipmentSlot::Head, EquippableSlot::Head)
            | (EquipmentSlot::Body, EquippableSlot::Body)
            | (EquipmentSlot::Saddle, EquippableSlot::Saddle)
    )
}

fn aabb_contains_any_liquid(world: &Arc<World>, aabb: WorldAabb) -> bool {
    let min_x = aabb.min_x().floor() as i32;
    let max_x = aabb.max_x().ceil() as i32;
    let min_y = aabb.min_y().floor() as i32;
    let max_y = aabb.max_y().ceil() as i32;
    let min_z = aabb.min_z().floor() as i32;
    let max_z = aabb.max_z().ceil() as i32;

    for x in min_x..max_x {
        for y in min_y..max_y {
            for z in min_z..max_z {
                if !get_fluid_state(world, BlockPos::new(x, y, z)).is_empty() {
                    return true;
                }
            }
        }
    }

    false
}

enum BlockEffectSegmentResult {
    Complete(i32),
    IterationLimit,
    Removed,
}

#[derive(Debug, Clone, Copy)]
struct BlockEffectFireSnapshot {
    was_on_fire: bool,
    was_freezing: bool,
    previous_remaining_fire_ticks: i32,
}

impl BlockEffectFireSnapshot {
    fn from_entity(entity: &dyn Entity) -> Self {
        Self {
            was_on_fire: entity.is_on_fire(),
            was_freezing: entity.is_freezing(),
            previous_remaining_fire_ticks: entity.remaining_fire_ticks(),
        }
    }
}

fn finish_inside_block_effects(
    entity: &dyn Entity,
    effect_collector: &mut InsideBlockEffectCollector,
    before_effects: BlockEffectFireSnapshot,
) {
    effect_collector.apply_and_clear(entity);
    if entity.is_removed() {
        return;
    }

    if is_in_rain(entity) {
        entity.clear_fire();
    }

    let extinguished = before_effects.was_on_fire && !entity.is_on_fire()
        || before_effects.was_freezing && !entity.is_freezing();
    if extinguished {
        entity.play_entity_on_fire_extinguished_sound();
    }

    let ignited_this_tick =
        entity.remaining_fire_ticks() > before_effects.previous_remaining_fire_ticks;
    if !entity.is_on_fire() && !ignited_this_tick {
        entity.set_remaining_fire_ticks(-entity.fire_immune_ticks());
    } else {
        entity.sync_base_fire_freeze_entity_data();
    }
}

fn is_in_rain(entity: &dyn Entity) -> bool {
    let Some(world) = entity.level() else {
        return false;
    };

    let pos = entity.block_position();
    world.is_raining_at(pos)
        || world.is_raining_at(BlockPos::new(
            pos.x(),
            entity.bounding_box().max_y().floor() as i32,
            pos.z(),
        ))
}

fn closest_open_space_direction(
    block_pos: BlockPos,
    fractional_position: DVec3,
    mut is_full_collision_block: impl FnMut(BlockPos) -> bool,
) -> Direction {
    let mut closest_direction = Direction::Up;
    let mut closest_distance = f64::MAX;

    for direction in MOVE_TOWARDS_CLOSEST_SPACE_DIRECTIONS {
        let neighbor_pos = direction.relative(block_pos);
        if is_full_collision_block(neighbor_pos) {
            continue;
        }

        let axis_delta = axis_component(fractional_position, direction.axis());
        let oriented_delta = if direction_step(direction) > 0.0 {
            1.0 - axis_delta
        } else {
            axis_delta
        };

        if oriented_delta < closest_distance {
            closest_distance = oriented_delta;
            closest_direction = direction;
        }
    }

    closest_direction
}

const fn axis_component(vector: DVec3, axis: Axis) -> f64 {
    match axis {
        Axis::X => vector.x,
        Axis::Y => vector.y,
        Axis::Z => vector.z,
    }
}

const fn direction_step(direction: Direction) -> f64 {
    match direction {
        Direction::Down | Direction::North | Direction::West => -1.0,
        Direction::Up | Direction::South | Direction::East => 1.0,
    }
}

fn fall_damage_reset_clip_target(
    position: DVec3,
    movement: DVec3,
    fall_distance: f64,
) -> Option<DVec3> {
    if fall_distance == 0.0 || movement.length_squared() < 1.0 {
        return None;
    }

    let check_distance = movement.length().min(8.0);
    Some(position + movement.normalize() * check_distance)
}

fn trapdoor_usable_as_ladder_state(
    trapdoor_state: BlockStateId,
    below_state: BlockStateId,
) -> bool {
    if trapdoor_state.try_get_value(&BlockStateProperties::OPEN) != Some(true) {
        return false;
    }

    below_state.get_block() == &vanilla_blocks::LADDER
        && below_state.try_get_value(&BlockStateProperties::FACING)
            == trapdoor_state.try_get_value(&BlockStateProperties::FACING)
}

pub(crate) fn get_input_vector(input: DVec3, speed: f32, yaw_degrees: f32) -> DVec3 {
    if input.length_squared() < 1.0E-7 {
        return DVec3::ZERO;
    }

    let movement = if input.length_squared() > 1.0 {
        input.normalize()
    } else {
        input
    } * f64::from(speed);
    let yaw = yaw_degrees.to_radians();
    let sin = yaw.sin();
    let cos = yaw.cos();
    DVec3::new(
        movement.x * f64::from(cos) - movement.z * f64::from(sin),
        movement.y,
        movement.z * f64::from(cos) + movement.x * f64::from(sin),
    )
}

fn collided_with_fluid(
    world: &Arc<World>,
    fluid_state: FluidState,
    block_pos: BlockPos,
    from: DVec3,
    to: DVec3,
    entity: &dyn Entity,
) -> bool {
    if fluid_state.is_empty() {
        return false;
    }

    let fluid_height = f64::from(get_height(world, block_pos, fluid_state));
    let fluid_box = WorldAabb::new(
        f64::from(block_pos.x()),
        f64::from(block_pos.y()),
        f64::from(block_pos.z()),
        f64::from(block_pos.x() + 1),
        f64::from(block_pos.y()) + fluid_height,
        f64::from(block_pos.z() + 1),
    );

    block_effects::collided_with_aabb_moving_from(
        entity.make_bounding_box_at(from),
        from,
        to,
        fluid_box,
    )
}

fn physics_state_for_move(entity: &dyn Entity) -> EntityPhysicsState {
    entity.base().physics_state(base::EntityPhysicsStateInput {
        max_up_step: entity.max_up_step(),
        backs_off_from_edge: entity.backs_off_from_edge(),
        descending: entity.is_descending(),
        can_walk_on_powder_snow: entity.can_walk_on_powder_snow(),
        is_falling_block: entity.entity_type() == &vanilla_entities::FALLING_BLOCK,
    })
}

/// Allocates a new unique entity ID.
///
/// This is the primary way to get entity IDs for spawning entities.
/// Thread-safe through the shared counter lock.
#[must_use]
pub fn next_entity_id() -> i32 {
    let mut counter = ENTITY_COUNTER.lock();
    let id = *counter;
    *counter = counter.wrapping_add(1);
    id
}

fn apply_block_effect_segment(
    entity: &dyn Entity,
    world: &Arc<World>,
    from: DVec3,
    to: DVec3,
    max_iterations: i32,
    effect_collector: &mut InsideBlockEffectCollector,
    visited_blocks: &mut FxHashSet<BlockPos>,
) -> BlockEffectSegmentResult {
    let aabb = entity.make_bounding_box_at(to).deflate(1.0E-5);
    if aabb.is_empty() {
        return BlockEffectSegmentResult::Complete(0);
    }

    let mut hit_iteration_limit = false;
    let Some(iterations) =
        block_effects::for_each_block_intersected_between(from, to, aabb, |pos, iteration| {
            if entity.is_removed() {
                return false;
            }
            if iteration >= max_iterations {
                hit_iteration_limit = true;
                return false;
            }

            let state = world.get_block_state(pos);
            if state.is_air() {
                return true;
            }

            let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
            let fluid_state = state.get_fluid_state();
            let entity_inside_shape =
                behavior.get_entity_inside_collision_shape(state, world.as_ref(), pos, entity);
            let inside_block = block_effects::collided_with_shape_moving_from(
                entity.make_bounding_box_at(from),
                from,
                to,
                pos,
                entity_inside_shape,
            );
            let inside_fluid = collided_with_fluid(world, fluid_state, pos, from, to, entity);

            if !(inside_block || inside_fluid) || !visited_blocks.insert(pos) {
                return true;
            }

            if inside_block {
                let moved_far = from.distance_squared(to) > 0.999_990_000_000_252_6_f64.powi(2);
                let is_precise = moved_far || aabb.intersects_block(pos);
                effect_collector.advance_step(iteration);
                behavior.entity_inside(state, world, pos, entity, effect_collector, is_precise);
                if entity.is_removed() {
                    return false;
                }
            }

            if inside_fluid {
                effect_collector.advance_step(iteration);
                FLUID_BEHAVIORS
                    .get_behavior(fluid_state.fluid_id)
                    .entity_inside(world, pos, entity, effect_collector);
            }
            !entity.is_removed()
        })
    else {
        if entity.is_removed() {
            return BlockEffectSegmentResult::Removed;
        }
        return if hit_iteration_limit {
            BlockEffectSegmentResult::IterationLimit
        } else {
            BlockEffectSegmentResult::Complete(0)
        };
    };

    if entity.is_removed() {
        BlockEffectSegmentResult::Removed
    } else {
        BlockEffectSegmentResult::Complete(iterations)
    }
}

fn relative_on_axis(position: DVec3, axis: Axis, amount: f64) -> DVec3 {
    match axis {
        Axis::X => DVec3::new(position.x + amount, position.y, position.z),
        Axis::Y => DVec3::new(position.x, position.y + amount, position.z),
        Axis::Z => DVec3::new(position.x, position.y, position.z + amount),
    }
}

fn record_movement_for_block_effects(
    entity: &dyn Entity,
    from: DVec3,
    to: DVec3,
    requested_movement: DVec3,
    actual_movement: DVec3,
) {
    if should_apply_resolved_movement(requested_movement, actual_movement) {
        entity.base().record_movement_this_tick(
            EntityMovement::with_axis_dependent_original_movement(from, to, requested_movement),
        );
    }
}

fn should_apply_resolved_movement(requested_movement: DVec3, actual_movement: DVec3) -> bool {
    let movement_length = actual_movement.length_squared();
    movement_length > MOVEMENT_RECORD_EPSILON
        || requested_movement.length_squared() - movement_length < MOVEMENT_RECORD_EPSILON
}

fn apply_step_on_block(entity: &dyn Entity, world: &Arc<World>) {
    if !entity.on_ground() {
        return;
    }

    let Some(effect_pos) = entity.on_pos_legacy() else {
        return;
    };
    let effect_state = world.get_block_state(effect_pos);
    let behavior = BLOCK_BEHAVIORS.get_behavior(effect_state.get_block());
    behavior.step_on(effect_state, world, effect_pos, entity);
}

#[expect(
    clippy::too_many_lines,
    reason = "vanilla movement block-effect traversal is easier to audit when kept in one sweep"
)]
fn apply_effects_from_block_movements(entity: &dyn Entity, movements: &[EntityMovement]) {
    if !entity.is_affected_by_blocks() {
        return;
    }

    let Some(world) = entity.level() else {
        return;
    };

    apply_step_on_block(entity, &world);

    let mut visited_blocks = FxHashSet::default();
    let mut effect_collector = InsideBlockEffectCollector::new();
    let before_effects = BlockEffectFireSnapshot::from_entity(entity);
    for movement in movements.iter().copied() {
        let mut remaining_iterations = 16;
        let delta = movement.to() - movement.from();
        if let Some(original_movement) = movement.axis_dependent_original_movement()
            && delta.length_squared() > 0.0
        {
            let mut segment_from = movement.from();
            for axis in block_effects::axis_step_order(original_movement) {
                let axis_move = block_effects::component(delta, axis);
                if axis_move == 0.0 {
                    continue;
                }

                let segment_to = relative_on_axis(segment_from, axis, axis_move);
                match apply_block_effect_segment(
                    entity,
                    &world,
                    segment_from,
                    segment_to,
                    remaining_iterations,
                    &mut effect_collector,
                    &mut visited_blocks,
                ) {
                    BlockEffectSegmentResult::Complete(iterations) => {
                        remaining_iterations -= iterations;
                    }
                    BlockEffectSegmentResult::IterationLimit => {
                        apply_block_effect_segment(
                            entity,
                            &world,
                            movement.to(),
                            movement.to(),
                            1,
                            &mut effect_collector,
                            &mut visited_blocks,
                        );
                        finish_inside_block_effects(entity, &mut effect_collector, before_effects);
                        return;
                    }
                    BlockEffectSegmentResult::Removed => {
                        finish_inside_block_effects(entity, &mut effect_collector, before_effects);
                        return;
                    }
                }
                segment_from = segment_to;
            }
        } else {
            match apply_block_effect_segment(
                entity,
                &world,
                movement.from(),
                movement.to(),
                remaining_iterations,
                &mut effect_collector,
                &mut visited_blocks,
            ) {
                BlockEffectSegmentResult::Complete(iterations) => {
                    remaining_iterations -= iterations;
                }
                BlockEffectSegmentResult::IterationLimit => {
                    apply_block_effect_segment(
                        entity,
                        &world,
                        movement.to(),
                        movement.to(),
                        1,
                        &mut effect_collector,
                        &mut visited_blocks,
                    );
                    finish_inside_block_effects(entity, &mut effect_collector, before_effects);
                    return;
                }
                BlockEffectSegmentResult::Removed => {
                    finish_inside_block_effects(entity, &mut effect_collector, before_effects);
                    return;
                }
            }
        }

        if remaining_iterations <= 0 {
            apply_block_effect_segment(
                entity,
                &world,
                movement.to(),
                movement.to(),
                1,
                &mut effect_collector,
                &mut visited_blocks,
            );
            finish_inside_block_effects(entity, &mut effect_collector, before_effects);
            return;
        }
    }

    finish_inside_block_effects(entity, &mut effect_collector, before_effects);
}

pub mod attribute;
mod base;
mod block_effects;
mod callback;
pub mod damage;
pub mod entities;
mod fluid_contact;
mod inside_block_effects;
mod living_base;
mod manager;
mod movement_sync;
mod registry;
mod shared_flags;
mod storage;
mod synced_data;
mod ticking;
mod tracker;

use crate::portal::TeleportTransition;
pub use base::{
    DEFAULT_TICKS_REQUIRED_TO_FREEZE, EntityAmethystStepSound, EntityBase, EntityBaseLoad,
    EntityBaseState, EntityFireFreezeState, EntityGroundContact, EntityMovement,
    EntityMovementEmission, EntityMovementFlags, EntityMovementProgress,
    EntityVerticalMovementStateUpdate,
};
pub use callback::{
    EntityChunkCallback, EntityLevelCallback, InactiveEntityCallback, NullEntityCallback,
    PlayerEntityCallback, RemovalReason,
};
pub use fluid_contact::EntityFluidContact;
pub use inside_block_effects::{
    InsideBlockEffectCallback, InsideBlockEffectCollector, InsideBlockEffectType,
};
pub use living_base::{ActiveMobEffect, DEATH_DURATION, LivingEntityBase, LivingTravelInput};
pub use manager::{
    AddEntityError, ChunkEntityLoadResult, EntityMoveError, EntityMoveUpdate, EntityOwnership,
    WorldEntityManager,
};
pub use movement_sync::{
    EntityMovementSyncPacket, EntityMovementSyncPackets, EntityMovementSyncState,
    EntityMovementSyncUpdate, EntityPositionRotSyncPacket, EntityPositionSyncDecision,
    EntityPositionSyncPacket, EntityPositionSyncSnapshot, EntityPositionSyncState,
    EntityRotationSyncState, EntityVelocitySyncState, POSITION_SYNC_THRESHOLD,
    PackedEntityRotation, ServerEntityMovementSyncState, ServerEntityMovementSyncUpdate,
};
pub use registry::{ENTITIES, EntityLoadRequest, EntityRegistry, init_entities};
pub(crate) use shared_flags::EntitySharedFlags;
pub(crate) use storage::EntityStorage;
pub use synced_data::EntitySyncedData;
pub(crate) use ticking::tick_vehicle_passengers_with_ticked_if;
pub use tracker::EntityTracker;

/// Type alias for a shared entity reference.
pub type SharedEntity = Arc<dyn Entity>;

/// Type alias for a weak entity reference.
pub type WeakEntity = Weak<dyn Entity>;

/// Final state accepted from a client-authored movement packet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcceptedClientMovement {
    /// Optional accepted packet position. Rotation-only packets leave this unset.
    pub position: Option<DVec3>,
    /// Accepted yaw and pitch in degrees.
    pub rotation: (f32, f32),
    /// Accepted on-ground flag.
    pub on_ground: bool,
    /// Accepted horizontal-collision flag.
    pub horizontal_collision: bool,
    /// Movement delta from the server position before processing the packet.
    pub movement: DVec3,
    /// Whether vanilla resets fall distance after the movement is applied.
    pub reset_fall_distance: bool,
}

/// Result of applying accepted client-authored movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptedClientMovementOutcome {
    /// Movement applied and regular post-processing should continue.
    Applied,
    /// Movement applied, but follow-up processing should stop because the
    /// entity handled a terminal side effect such as death.
    Handled,
}

/// Object-safe access to an entity trait object from default `Entity` methods.
pub trait EntityEventSource {
    /// Returns this entity as a game-event source.
    fn as_entity_event_source(&self) -> &dyn Entity;
}

impl<T: Entity> EntityEventSource for T {
    fn as_entity_event_source(&self) -> &dyn Entity {
        self
    }
}

/// A trait for entities.
///
/// This trait provides the core functionality for entities.
/// It's based on Minecraft's `Entity` class.
///
/// # Using `EntityBase`
///
/// Entities expose [`EntityBase`] to get default implementations for common
/// methods like `id()`, `uuid()`, `position()`, etc.
///
/// ```ignore
/// impl Entity for MyEntity {
///     fn base(&self) -> &EntityBase { &self.base }
///     fn entity_type(&self) -> EntityTypeRef { vanilla_entities::MY_ENTITY }
///     fn bounding_box(&self) -> WorldAabb { /* ... */ }
///     // All other common methods use defaults from EntityBase!
/// }
/// ```
pub trait Entity: EntityEventSource + Send + Sync {
    /// Returns a reference to the entity's shared vanilla base fields.
    fn base(&self) -> &EntityBase;

    /// Gets the entity type containing tracking range, dimensions, etc.
    fn entity_type(&self) -> EntityTypeRef;

    /// Returns whether this entity should be broadcast to the given player.
    ///
    /// Mirrors vanilla `Entity.broadcastToPlayer`. Most entities are always
    /// broadcastable; players override this for spectator visibility rules.
    fn broadcast_to_player(&self, _player: &Player) -> bool {
        true
    }

    /// Gets the entity's unique network ID (session-local).
    fn id(&self) -> i32 {
        self.base().id()
    }

    /// Gets the UUID of the entity (persistent identifier).
    fn uuid(&self) -> Uuid {
        self.base().uuid()
    }

    /// Gets the entity's current position.
    fn position(&self) -> DVec3 {
        self.base().position()
    }

    /// Gets the entity's current block position.
    fn block_position(&self) -> BlockPos {
        let position = self.position();
        BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        )
    }

    /// Returns vanilla `Entity.getInBlockState`.
    fn in_block_state(&self, world: &World) -> BlockStateId {
        self.base().in_block_state(world)
    }

    /// Gets the entity position used by vanilla movement traces.
    fn old_position(&self) -> DVec3 {
        self.base().old_position()
    }

    /// Gets the entity's bounding box for collision queries.
    fn bounding_box(&self) -> WorldAabb {
        self.base().bounding_box()
    }

    /// Returns vanilla `Entity.isFree()` for the current bounding box shifted by `delta`.
    fn is_free(&self, delta: DVec3) -> bool {
        let Some(world) = self.level() else {
            return false;
        };

        let target_box = self.bounding_box().move_vec(delta);
        let collision_world =
            WorldCollisionProvider::for_entity(&world, self.as_entity_event_source());
        if collision_world.has_collision_with_context(
            &target_box.deflate(COLLISION_EPSILON),
            physics_state_for_move(self.as_entity_event_source()).block_collision_context(),
        ) {
            return false;
        }

        !aabb_contains_any_liquid(&world, target_box)
    }

    /// Returns whether this entity obstructs block placement.
    ///
    /// Mirrors vanilla `Entity.blocksBuilding`. Base entities do not obstruct
    /// placement unless a concrete entity type opts in.
    fn blocks_building(&self) -> bool {
        false
    }

    /// Returns whether this entity can be targeted by picking and interaction raycasts.
    ///
    /// Mirrors vanilla `Entity.isPickable`. Base entities are not pickable unless
    /// a concrete entity type opts in.
    fn is_pickable(&self) -> bool {
        false
    }

    /// Returns whether this entity participates in vanilla push separation.
    ///
    /// Mirrors vanilla `Entity.isPushable`. Base entities are not pushable unless
    /// a concrete entity type opts in.
    fn is_pushable(&self) -> bool {
        false
    }

    /// Returns whether vanilla fluid currents can push this entity.
    fn is_pushed_by_fluid(&self) -> bool {
        true
    }

    /// Returns whether this entity is invisible to normal entity selectors.
    ///
    /// Mirrors vanilla `Entity.isSpectator`. Base entities are never spectators;
    /// players override this from their game mode.
    fn is_spectator(&self) -> bool {
        false
    }

    /// Returns true for vanilla players whose abilities have `flying` set.
    fn is_flying_player(&self) -> bool {
        false
    }

    /// Returns whether `other` can collide with this entity.
    ///
    /// Mirrors vanilla `Entity.canBeCollidedWith`. Base entities cannot be collided
    /// with unless a concrete entity type opts in.
    fn can_be_collided_with(&self, _other: Option<&dyn Entity>) -> bool {
        false
    }

    /// Returns whether projectile collision may interact with this entity.
    ///
    /// Mirrors vanilla `Entity.canBeHitByProjectile`.
    fn can_be_hit_by_projectile(&self) -> bool {
        !self.is_removed() && self.is_pickable()
    }

    /// Gets the vehicle this entity is riding, if present.
    ///
    /// Mirrors vanilla `Entity.getVehicle`.
    fn vehicle(&self) -> Option<SharedEntity> {
        self.base().vehicle()
    }

    /// Returns whether this entity is riding another entity.
    ///
    /// Mirrors vanilla `Entity.isPassenger`.
    fn is_passenger(&self) -> bool {
        self.vehicle().is_some()
    }

    /// Stops riding the current vehicle, if any.
    ///
    /// Mirrors vanilla `Entity.stopRiding`.
    fn stop_riding(&self) {
        self.base().stop_riding();
    }

    /// Gets this entity's direct passengers.
    ///
    /// Mirrors vanilla `Entity.getPassengers`.
    fn passengers(&self) -> Vec<SharedEntity> {
        self.base().passengers()
    }

    /// Counts indirect player passengers.
    ///
    /// Mirrors vanilla `Entity.countPlayerPassengers`.
    fn count_player_passengers(&self) -> usize {
        fn count_passenger_tree(
            passengers: Vec<SharedEntity>,
            visited: &mut FxHashSet<i32>,
        ) -> usize {
            let mut total = 0;
            for passenger in passengers {
                if !visited.insert(passenger.id()) {
                    continue;
                }
                if passenger.entity_type() == &vanilla_entities::PLAYER {
                    total += 1;
                }
                total += count_passenger_tree(passenger.passengers(), visited);
            }
            total
        }

        let mut visited = FxHashSet::default();
        visited.insert(self.id());
        count_passenger_tree(self.passengers(), &mut visited)
    }

    /// Returns whether this entity has exactly one indirect player passenger.
    ///
    /// Mirrors vanilla `Entity.hasExactlyOnePlayerPassenger`.
    fn has_exactly_one_player_passenger(&self) -> bool {
        self.count_player_passengers() == 1
    }

    /// Gets this entity's first direct passenger.
    ///
    /// Mirrors vanilla `Entity.getFirstPassenger`.
    fn first_passenger(&self) -> Option<SharedEntity> {
        self.base().first_passenger()
    }

    /// Returns the living passenger currently controlling this entity, if any.
    ///
    /// Mirrors vanilla `Entity.getControllingPassenger`. Base entities have no
    /// controller; controllable vehicles override this based on their own rules.
    fn controlling_passenger(&self) -> Option<SharedEntity> {
        None
    }

    /// Returns whether this entity currently has a controlling passenger.
    ///
    /// Mirrors vanilla `Entity.hasControllingPassenger`.
    fn has_controlling_passenger(&self) -> bool {
        self.controlling_passenger().is_some()
    }

    /// Returns whether this entity has any direct passengers.
    ///
    /// Mirrors vanilla `Entity.isVehicle`.
    fn is_vehicle(&self) -> bool {
        self.base().is_vehicle()
    }

    /// Returns whether `passenger` is a direct passenger of this entity.
    ///
    /// Mirrors vanilla `Entity.hasPassenger(Entity)`.
    fn has_passenger(&self, passenger: &dyn Entity) -> bool {
        self.base().has_passenger_id(passenger.id())
    }

    /// Returns the current direct passenger index for attachment lookup.
    fn passenger_index(&self, passenger: &dyn Entity) -> Option<usize> {
        self.passengers()
            .iter()
            .position(|entity| entity.id() == passenger.id())
    }

    /// Returns this passenger's vehicle attachment point.
    ///
    /// Mirrors vanilla `Entity.getVehicleAttachmentPoint`.
    fn vehicle_attachment_point(&self, _vehicle: &dyn Entity) -> DVec3 {
        let dimensions = self.base().dimensions();
        dimensions.attachments.get_clamped(
            EntityAttachment::Vehicle,
            0,
            self.rotation().0,
            dimensions,
        )
    }

    /// Returns this vehicle's passenger attachment point.
    ///
    /// Mirrors vanilla `Entity.getPassengerAttachmentPoint` for the base entity class.
    fn passenger_attachment_point(&self, passenger: &dyn Entity) -> DVec3 {
        let dimensions = self.base().dimensions();
        let passenger_index = self.passenger_index(passenger).unwrap_or_default();
        dimensions.attachments.get_clamped(
            EntityAttachment::Passenger,
            passenger_index,
            self.rotation().0,
            dimensions,
        )
    }

    /// Returns the world position where `passenger` should ride this vehicle.
    ///
    /// Mirrors vanilla `Entity.getPassengerRidingPosition`.
    fn passenger_riding_position(&self, passenger: &dyn Entity) -> DVec3 {
        self.position() + self.passenger_attachment_point(passenger)
    }

    /// Repositions a direct passenger from this vehicle's attachment point.
    ///
    /// Mirrors vanilla `Entity.positionRider`.
    fn position_rider(&self, passenger: &dyn Entity) {
        if !self.has_passenger(passenger) {
            return;
        }

        let riding_position = self.passenger_riding_position(passenger);
        let vehicle_attachment = passenger.vehicle_attachment_point(self.as_entity_event_source());
        if let Err(error) = passenger.try_set_position(riding_position - vehicle_attachment) {
            log::debug!(
                "Failed to position passenger {} riding entity {}: {error}",
                passenger.id(),
                self.id()
            );
        }
    }

    /// Returns this entity's root vehicle ID, or this entity's ID when it is not riding.
    ///
    /// Mirrors vanilla `Entity.getRootVehicle` using session IDs for object identity.
    fn root_vehicle_id(&self) -> i32 {
        self.root_vehicle().map_or(self.id(), |entity| entity.id())
    }

    /// Returns this entity's root vehicle, if this entity is riding one.
    ///
    /// Mirrors vanilla `Entity.getRootVehicle`.
    fn root_vehicle(&self) -> Option<SharedEntity> {
        let mut root = self.vehicle()?;
        let mut visited = FxHashSet::default();
        visited.insert(self.id());

        loop {
            if !visited.insert(root.id()) {
                return Some(root);
            }
            let Some(next) = root.vehicle() else {
                return Some(root);
            };
            root = next;
        }
    }

    /// Returns whether this entity and `other` share the same root vehicle.
    ///
    /// Mirrors vanilla `Entity.isPassengerOfSameVehicle`.
    fn is_passenger_of_same_vehicle(&self, other: &dyn Entity) -> bool {
        self.root_vehicle_id() == other.root_vehicle_id()
    }

    /// Returns whether `entity` is an indirect passenger of this entity.
    ///
    /// Mirrors vanilla `Entity.hasIndirectPassenger`.
    fn has_indirect_passenger(&self, entity: &dyn Entity) -> bool {
        let target_id = self.id();
        let mut vehicle = entity.vehicle();
        let mut visited = Vec::new();

        while let Some(ridden) = vehicle {
            let ridden_id = ridden.id();
            if ridden_id == target_id {
                return true;
            }
            if visited.contains(&ridden_id) {
                return false;
            }
            visited.push(ridden_id);
            vehicle = ridden.vehicle();
        }

        false
    }

    /// Returns whether this entity can collide with `other`.
    ///
    /// Mirrors vanilla `Entity.canCollideWith`.
    fn can_collide_with(&self, other: &dyn Entity) -> bool {
        other.can_be_collided_with(Some(self.as_entity_event_source()))
            && !self.is_passenger_of_same_vehicle(other)
    }

    /// Adds an impulse to this entity's velocity and marks velocity for sync.
    ///
    /// Mirrors vanilla `Entity.push(double, double, double)`.
    fn push_impulse(&self, impulse: DVec3) {
        if !impulse.is_finite() {
            return;
        }

        self.set_velocity(self.velocity() + impulse);
        self.mark_velocity_sync();
    }

    /// Applies vanilla entity-to-entity push separation.
    ///
    /// Mirrors vanilla `Entity.push(Entity)`.
    fn push_entity(&self, entity: &dyn Entity) {
        if self.is_passenger_of_same_vehicle(entity) || entity.no_physics() || self.no_physics() {
            return;
        }

        let mut x = entity.position().x - self.position().x;
        let mut z = entity.position().z - self.position().z;
        let mut distance = x.abs().max(z.abs());
        if distance < 0.01 {
            return;
        }

        distance = distance.sqrt();
        x /= distance;
        z /= distance;
        let scale = (1.0 / distance).min(1.0) * 0.05;
        x *= scale;
        z *= scale;

        if !self.is_vehicle() && self.is_pushable() {
            self.push_impulse(DVec3::new(-x, 0.0, -z));
        }
        if !entity.is_vehicle() && entity.is_pushable() {
            entity.push_impulse(DVec3::new(x, 0.0, z));
        }
    }

    /// Builds this entity's default bounding box at `position`.
    fn make_bounding_box_at(&self, position: DVec3) -> WorldAabb {
        let dimensions = self.base().dimensions();
        WorldAabb::entity_box(
            position.x,
            position.y,
            position.z,
            f64::from(dimensions.half_width()),
            f64::from(dimensions.height),
        )
    }

    /// Default vanilla `Entity.tick()` behavior.
    ///
    /// Concrete entity ticks that mirror vanilla `super.tick()` should call this
    /// rather than calling [`Self::base_tick`] directly.
    fn default_tick(&self) {
        self.base_tick();
    }

    /// Called every game tick when the entity is in a ticked chunk.
    ///
    /// Use `self.level()` to access the world for physics, block queries, etc.
    /// The caller handles post-tick dirty data sync.
    ///
    /// Steel keeps the fallback empty because many vanilla subclasses override
    /// tick without calling `super.tick()`.
    fn tick(&self) {}

    /// Called every game tick while this entity is riding another entity.
    ///
    /// Mirrors vanilla `Entity.rideTick`.
    fn ride_tick(&self) {
        self.set_velocity(DVec3::ZERO);
        self.tick();
        if let Some(vehicle) = self.vehicle() {
            vehicle.position_rider(self.as_entity_event_source());
        }
    }

    /// Runs the vanilla base-tick physics pieces Steel currently implements.
    ///
    /// This intentionally stays separate from `tick()` because several vanilla
    /// subclasses override tick without calling `super.tick()`.
    fn base_tick(&self) {
        self.base().advance_base_tick_state();
        self.base().advance_powder_snow_contact_for_base_tick();
        self.refresh_fluid_contact_for_base_tick();
        self.base().reset_fall_distance_in_water();
        if self
            .base()
            .advance_fire_tick(self.fire_immune(), self.is_in_lava())
        {
            self.hurt(
                &DamageSource::environment(&vanilla_damage_types::ON_FIRE),
                1.0,
            );
        }
        self.base().dampen_fall_distance_in_lava();
        self.check_below_world();
        self.sync_base_fire_freeze_entity_data();
        // TODO: Add remaining vanilla baseTick pieces: portal, sprint particles, and leash tick.
    }

    /// Applies vanilla below-world handling.
    fn check_below_world(&self) {
        let Some(world) = self.level() else {
            return;
        };

        if self.position().y < f64::from(world.get_min_y() - 64) {
            self.on_below_world();
        }
    }

    /// Runs entity-specific behavior after falling below the world.
    fn on_below_world(&self) {
        self.set_removed(RemovalReason::Discarded);
    }

    /// Applies an inside-block effect queued by vanilla's step-based collector.
    fn apply_inside_block_effect(&self, effect_type: InsideBlockEffectType) {
        let fire_ignite_extra_ticks = if matches!(effect_type, InsideBlockEffectType::FireIgnite) {
            self.fire_ignite_extra_ticks()
        } else {
            0
        };
        self.base().apply_inside_block_effect(
            effect_type,
            self.can_freeze(),
            self.fire_immune(),
            fire_ignite_extra_ticks,
            self.ticks_required_to_freeze(),
            self.remaining_fire_ticks_cap(),
        );
        self.sync_base_fire_freeze_entity_data();
    }

    /// Gets the world this entity is in.
    ///
    /// Returns `None` if the entity is not in a world or the world was dropped.
    /// Mirrors vanilla's `Entity.level()`.
    fn level(&self) -> Option<Arc<World>> {
        self.base().level()
    }

    /// Packs dirty entity data for network synchronization.
    ///
    /// Returns `Some(values)` if there are dirty values to sync, `None` otherwise.
    /// Clears the dirty flags after packing.
    fn pack_dirty_entity_data(&self) -> Option<Vec<DataValue>> {
        self.synced_data().and_then(EntitySyncedData::pack_dirty)
    }

    /// Packs all non-default entity data for initial spawn.
    ///
    /// Used when sending entity data to a player who just started tracking this entity.
    fn pack_all_entity_data(&self) -> Vec<DataValue> {
        self.synced_data()
            .map_or_else(Vec::new, EntitySyncedData::pack_all)
    }

    /// Returns the synchronized entity-data container for entities with vanilla data accessors.
    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        None
    }

    /// Packs syncable attributes for initial spawn pairing.
    ///
    /// Mirrors vanilla `ServerEntity.sendPairingData`, which sends all syncable
    /// living attributes after the add-entity and metadata packets.
    fn pack_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        Vec::new()
    }

    /// Drains syncable dirty attributes for per-tick tracking updates.
    ///
    /// Mirrors vanilla `ServerEntity.sendDirtyEntityData`, which sends dirty
    /// living attributes after dirty entity data.
    fn drain_dirty_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        Vec::new()
    }

    /// Returns true if the entity has been marked for removal.
    fn is_removed(&self) -> bool {
        self.base().is_removed()
    }

    /// Returns whether this entity is alive for vanilla generic entity checks.
    fn is_alive(&self) -> bool {
        !self.is_removed()
    }

    /// Returns why this entity was removed, if it has been removed.
    fn removal_reason(&self) -> Option<RemovalReason> {
        self.base().removal_reason()
    }

    /// Marks the entity as removed with the given reason.
    fn set_removed(&self, reason: RemovalReason) {
        self.base().set_removed(reason);
    }

    /// Sets the level callback for lifecycle events (movement, removal).
    fn set_level_callback(&self, callback: Arc<dyn EntityLevelCallback>) {
        self.base().set_level_callback(callback);
    }

    /// Gets the entity as an `ItemEntity` if it is one.
    fn as_item_entity(self: Arc<Self>) -> Option<Arc<ItemEntity>> {
        None
    }

    /// Returns true for entities that implement vanilla living-entity behavior.
    fn is_living_entity(&self) -> bool {
        false
    }

    /// Returns true when vanilla `ServerEntity` should force velocity sync for fall flying.
    fn forces_fall_flying_velocity_sync(&self) -> bool {
        false
    }

    /// Returns true when movement is driven by serverbound movement packets.
    fn uses_client_movement_packets(&self) -> bool {
        if !self.is_removed()
            && let Some(controller) = self.controlling_passenger()
            && controller.id() != self.id()
            && controller.uses_client_movement_packets()
        {
            return true;
        }

        false
    }

    /// Returns true when normal server ticks drive this entity's movement.
    fn is_server_driven_movement(&self) -> bool {
        !self.uses_client_movement_packets()
    }

    /// Returns true when vanilla allows this side to apply movement simulation side effects.
    fn can_simulate_movement(&self) -> bool {
        self.is_server_driven_movement()
    }

    /// Returns true when vanilla allows this side to run entity AI/travel logic.
    fn is_effective_ai(&self) -> bool {
        self.is_server_driven_movement()
    }

    /// Returns true when vanilla landing bounce should be suppressed.
    fn is_suppressing_bounce(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_shift_key_down)
    }

    /// Returns true when vanilla block step-on hooks should treat this entity as careful.
    fn is_stepping_carefully(&self) -> bool {
        self.is_suppressing_bounce()
    }

    /// Returns true when vanilla collision context should treat the entity as descending.
    fn is_descending(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_shift_key_down)
    }

    /// Sets the vanilla shift-key-down shared flag.
    fn set_shared_shift_key_down(&self, shift_key_down: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_shift_key_down(shift_key_down);
        }
    }

    /// Sets the vanilla swimming shared flag.
    fn set_shared_swimming(&self, swimming: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_swimming(swimming);
        }
    }

    /// Sets the vanilla sprinting shared flag.
    fn set_shared_sprinting(&self, sprinting: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_sprinting(sprinting);
        }
    }

    /// Sets the vanilla fall-flying shared flag.
    fn set_shared_fall_flying(&self, fall_flying: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_fall_flying(fall_flying);
        }
    }

    /// Returns vanilla `PowderSnowBlock.canEntityWalkOnPowderSnow`.
    fn default_can_walk_on_powder_snow(&self) -> bool {
        REGISTRY.entity_types.is_in_tag(
            self.entity_type(),
            &EntityTypeTag::POWDER_SNOW_WALKABLE_MOBS,
        )
    }

    /// Returns whether this entity can walk on powder snow.
    fn can_walk_on_powder_snow(&self) -> bool {
        self.default_can_walk_on_powder_snow()
    }

    /// Returns whether vanilla excludes this vehicle from floating kicks.
    fn is_flying_vehicle(&self) -> bool {
        false
    }

    /// Returns true if vanilla rules consider this entity to be on a climbable block.
    fn on_climbable(&self) -> bool {
        false
    }

    /// Returns the movement vector vanilla exposes for block-contact logic.
    fn known_movement(&self) -> DVec3 {
        if let Some(controller) = self.controlling_passenger()
            && !self.is_removed()
            && controller.entity_type() == &vanilla_entities::PLAYER
        {
            return controller.known_movement();
        }

        self.velocity()
    }

    /// Returns the base-tick displacement vanilla exposes as `getKnownSpeed`.
    fn known_speed(&self) -> DVec3 {
        if let Some(controller) = self.controlling_passenger()
            && !self.is_removed()
            && controller.entity_type() == &vanilla_entities::PLAYER
        {
            return controller.known_speed();
        }

        self.base().known_speed()
    }

    /// Returns vanilla `Entity.tickCount`.
    fn tick_count(&self) -> i32 {
        self.base().tick_count()
    }

    /// Advances vanilla `Entity.tickCount`.
    fn advance_tick_count(&self) {
        self.base().advance_tick_count();
    }

    /// Returns vanilla small and big fall sounds for this entity.
    fn fall_sounds(&self) -> (SoundEventRef, SoundEventRef) {
        (
            &sound_events::ENTITY_GENERIC_SMALL_FALL,
            &sound_events::ENTITY_GENERIC_BIG_FALL,
        )
    }

    /// Gets the entity's rotation as (yaw, pitch) in degrees.
    ///
    /// Yaw is horizontal rotation (0-360), pitch is vertical (-90 to 90).
    fn rotation(&self) -> (f32, f32) {
        self.base().rotation()
    }

    /// Sets the entity's rotation as (yaw, pitch) in degrees.
    fn set_rotation(&self, rotation: (f32, f32)) {
        self.base().set_rotation(rotation);
    }

    /// Extra spawn-packet data used by vanilla for entity-specific construction.
    fn spawn_data(&self) -> i32 {
        0
    }

    /// Gets the eye height for this entity.
    ///
    /// Default implementation returns the eye height from the entity type dimensions.
    /// Override for entities with pose-dependent eye heights (e.g., players).
    fn get_eye_height(&self) -> f64 {
        f64::from(self.base().dimensions().eye_height)
    }

    /// Returns vanilla `Entity.getFluidJumpThreshold()`.
    fn get_fluid_jump_threshold(&self) -> f64 {
        if self.get_eye_height() < 0.4 {
            0.0
        } else {
            0.4
        }
    }

    /// Gets the Y coordinate of the entity's eyes.
    ///
    /// Equivalent to vanilla's `Entity.getEyeY()`.
    fn get_eye_y(&self) -> f64 {
        self.position().y + self.get_eye_height()
    }

    /// Calculates vanilla `Entity.calculateViewVector()`.
    fn calculate_view_vector(&self, pitch_degrees: f32, yaw_degrees: f32) -> DVec3 {
        let pitch = pitch_degrees.to_radians();
        let yaw = -yaw_degrees.to_radians();
        let yaw_cos = yaw.cos();
        let yaw_sin = yaw.sin();
        let pitch_cos = pitch.cos();
        let pitch_sin = pitch.sin();
        DVec3::new(
            f64::from(yaw_sin * pitch_cos),
            f64::from(-pitch_sin),
            f64::from(yaw_cos * pitch_cos),
        )
    }

    /// Returns vanilla `Entity.getLookAngle()`.
    fn look_angle(&self) -> DVec3 {
        let (yaw, pitch) = self.rotation();
        self.calculate_view_vector(pitch, yaw)
    }

    /// Gets the entity's velocity in blocks per tick.
    fn velocity(&self) -> DVec3 {
        self.base().velocity()
    }

    /// Sets the entity's velocity.
    fn set_velocity(&self, velocity: DVec3) {
        self.base().set_velocity(velocity);
    }

    /// Returns true when vanilla `ServerEntity` should consider sending velocity.
    fn needs_velocity_sync(&self) -> bool {
        self.base().needs_velocity_sync()
    }

    /// Marks velocity for vanilla `ServerEntity` synchronization.
    fn mark_velocity_sync(&self) {
        self.base().mark_velocity_sync();
    }

    /// Clears the vanilla velocity sync marker after send processing.
    fn clear_velocity_sync(&self) {
        self.base().clear_velocity_sync();
    }

    /// Returns accumulated vanilla fall distance.
    fn fall_distance(&self) -> f64 {
        self.base().fall_distance()
    }

    /// Returns whether this entity is currently inside powder snow.
    fn is_in_powder_snow(&self) -> bool {
        self.base().is_in_powder_snow()
    }

    /// Returns whether this entity was inside powder snow during the previous base tick.
    fn was_in_powder_snow(&self) -> bool {
        self.base().was_in_powder_snow()
    }

    /// Sets accumulated vanilla fall distance.
    fn set_fall_distance(&self, fall_distance: f64) {
        self.base().set_fall_distance(fall_distance);
    }

    /// Resets accumulated vanilla fall distance.
    fn reset_fall_distance(&self) {
        self.base().reset_fall_distance();
    }

    /// Mirrors vanilla `Entity.checkFallDistanceAccumulation()`.
    fn check_fall_distance_accumulation(&self) {
        if self.velocity().y > -0.5 && self.fall_distance() > 1.0 {
            self.set_fall_distance(1.0);
        }
    }

    /// Returns the current vanilla fire/freeze state.
    fn fire_freeze_state(&self) -> EntityFireFreezeState {
        self.base().fire_freeze_state()
    }

    /// Returns vanilla `remainingFireTicks`.
    fn remaining_fire_ticks(&self) -> i32 {
        self.base().remaining_fire_ticks()
    }

    /// Sets vanilla `remainingFireTicks`.
    fn set_remaining_fire_ticks(&self, remaining_fire_ticks: i32) {
        self.base().set_remaining_fire_ticks(
            self.remaining_fire_ticks_cap()
                .map_or(remaining_fire_ticks, |cap| remaining_fire_ticks.min(cap)),
        );
        self.sync_base_fire_freeze_entity_data();
    }

    /// Returns synchronized vanilla `TicksFrozen`.
    fn ticks_frozen(&self) -> i32 {
        self.base().ticks_frozen()
    }

    /// Sets synchronized vanilla `TicksFrozen`.
    fn set_ticks_frozen(&self, ticks_frozen: i32) {
        self.base().set_ticks_frozen(ticks_frozen);
        self.sync_base_fire_freeze_entity_data();
    }

    /// Returns whether this entity is immune to fire effects and fire damage.
    fn fire_immune(&self) -> bool {
        self.entity_type().fire_immune
    }

    /// Returns vanilla fire immunity cooldown ticks after not being ignited.
    fn fire_immune_ticks(&self) -> i32 {
        0
    }

    /// Returns whether vanilla should play this entity's lava hurt sound.
    fn should_play_lava_hurt_sound(&self) -> bool {
        true
    }

    /// Applies vanilla lava-contact damage after lava ignition effects.
    fn lava_hurt(&self) {
        if self.fire_immune() {
            return;
        }

        if self.hurt(&DamageSource::environment(&vanilla_damage_types::LAVA), 4.0)
            && self.should_play_lava_hurt_sound()
        {
            let pitch = {
                let mut random = self.base().random().lock();
                2.0 + random.next_f32() * 0.4
            };
            self.play_sound(&sound_events::ENTITY_GENERIC_BURN, 0.4, pitch);
        }
    }

    /// Maximum vanilla `remainingFireTicks` this entity can store.
    fn remaining_fire_ticks_cap(&self) -> Option<i32> {
        None
    }

    /// Returns extra ticks added by fire-block ignition before 8-second ignition.
    fn fire_ignite_extra_ticks(&self) -> i32 {
        0
    }

    /// Returns whether the entity is on fire on the server.
    fn is_on_fire(&self) -> bool {
        self.base().is_on_fire(self.fire_immune())
    }

    /// Returns vanilla `hasVisualFire`.
    fn has_visual_fire(&self) -> bool {
        self.base().has_visual_fire()
    }

    /// Returns whether the entity has any frozen ticks.
    fn is_freezing(&self) -> bool {
        self.base().is_freezing()
    }

    /// Returns vanilla `Entity.canFreeze()` without living-equipment overrides.
    fn default_can_freeze(&self) -> bool {
        !REGISTRY.entity_types.is_in_tag(
            self.entity_type(),
            &EntityTypeTag::FREEZE_IMMUNE_ENTITY_TYPES,
        )
    }

    /// Returns whether this entity may accumulate frozen ticks.
    fn can_freeze(&self) -> bool {
        self.default_can_freeze()
    }

    /// Returns vanilla `getTicksRequiredToFreeze`.
    fn ticks_required_to_freeze(&self) -> i32 {
        DEFAULT_TICKS_REQUIRED_TO_FREEZE
    }

    /// Returns whether this entity has reached full-freeze duration.
    fn is_fully_frozen(&self) -> bool {
        self.base().is_fully_frozen(self.ticks_required_to_freeze())
    }

    /// Clears accumulated freezing.
    fn clear_freeze(&self) {
        self.base().clear_freeze();
        self.sync_base_fire_freeze_entity_data();
    }

    /// Clears fire without resetting the vanilla fire immunity cooldown.
    fn clear_fire(&self) {
        self.base().clear_fire();
        self.sync_base_fire_freeze_entity_data();
    }

    /// Ignites this entity for a vanilla tick duration.
    fn ignite_for_ticks(&self, number_of_ticks: i32) {
        self.base()
            .ignite_for_ticks(number_of_ticks, self.remaining_fire_ticks_cap());
        self.sync_base_fire_freeze_entity_data();
    }

    /// Projects base fire/freeze state into generated synced entity data.
    fn sync_base_fire_freeze_entity_data(&self) {
        let Some(synced_data) = self.synced_data() else {
            return;
        };

        synced_data.set_base_ticks_frozen(self.ticks_frozen());
        synced_data.set_base_on_fire_flag(self.is_on_fire() || self.has_visual_fire());
    }

    /// Returns true if this entity is currently touching water.
    fn is_in_water(&self) -> bool {
        self.fluid_contact().water_height() > 0.0
    }

    /// Returns true if this entity is currently touching lava.
    fn is_in_lava(&self) -> bool {
        self.fluid_contact().lava_height() > 0.0
    }

    /// Returns true if this entity's eyes are currently inside water.
    fn is_eye_in_water(&self) -> bool {
        self.fluid_contact().eye_in_water()
    }

    /// Returns true if this entity's eyes are currently inside lava.
    fn is_eye_in_lava(&self) -> bool {
        self.fluid_contact().eye_in_lava()
    }

    /// Returns vanilla underwater state.
    fn is_under_water(&self) -> bool {
        self.base().was_eye_in_water() && self.is_in_water()
    }

    /// Returns cached fluid contact from the last entity fluid refresh.
    fn fluid_contact(&self) -> EntityFluidContact {
        self.base().fluid_contact()
    }

    /// Refreshes cached fluid contact from this entity's current bounding box.
    fn refresh_fluid_contact(&self) -> EntityFluidContact {
        self.scan_and_store_fluid_contact(false)
    }

    /// Refreshes cached fluid contact with vanilla base-tick eye-water history.
    fn refresh_fluid_contact_for_base_tick(&self) -> EntityFluidContact {
        self.scan_and_store_fluid_contact(true)
    }

    /// Scans current fluid contact and stores it on the entity base.
    fn scan_and_store_fluid_contact(&self, advance_eye_water_history: bool) -> EntityFluidContact {
        let Some(world) = self.level() else {
            let contact = EntityFluidContact::default();
            if advance_eye_water_history {
                self.base().set_fluid_contact_for_base_tick(contact);
            } else {
                self.base().set_fluid_contact(contact);
            }
            return contact;
        };

        let contact = if advance_eye_water_history {
            EntityFluidContact::scan_with_currents(
                &world,
                self.position(),
                self.get_eye_y(),
                self.bounding_box(),
                self.is_pushed_by_fluid(),
            )
        } else {
            EntityFluidContact::scan(
                &world,
                self.position(),
                self.get_eye_y(),
                self.bounding_box(),
            )
        };
        if advance_eye_water_history {
            self.base().set_fluid_contact_for_base_tick(contact);
            self.apply_fluid_current_for_base_tick(&world, contact);
        } else {
            self.base().set_fluid_contact(contact);
        }
        contact
    }

    /// Applies vanilla water/lava current impulses from the base-tick fluid scan.
    fn apply_fluid_current_for_base_tick(&self, world: &Arc<World>, contact: EntityFluidContact) {
        if !self.is_pushed_by_fluid() {
            return;
        }

        let is_player = self.entity_type() == &vanilla_entities::PLAYER;
        let old_velocity = self.velocity();
        let water_impulse =
            contact.water_current_impulse(is_player, old_velocity, WATER_ENTITY_FLOW_SCALE);
        self.apply_fluid_current_impulse(water_impulse);

        let old_velocity = old_velocity + water_impulse;
        let lava_impulse = contact.lava_current_impulse(
            is_player,
            old_velocity,
            LavaFluid::entity_flow_scale(world),
        );
        self.apply_fluid_current_impulse(lava_impulse);
    }

    /// Applies a non-zero fluid current impulse and marks velocity sync.
    fn apply_fluid_current_impulse(&self, impulse: DVec3) {
        if impulse.length_squared() > 0.0 {
            self.push_impulse(impulse);
        }
    }

    /// Returns true if this entity type ignores vanilla fall damage.
    fn is_fall_damage_immune(&self) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::FALL_DAMAGE_IMMUNE)
    }

    /// Applies vanilla fall damage. Base entities only propagate to passengers.
    fn cause_fall_damage(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool {
        for passenger in self.passengers() {
            passenger.cause_fall_damage(fall_distance, damage_modifier, source);
        }

        false
    }

    /// Returns true if the entity is on the ground.
    fn on_ground(&self) -> bool {
        self.base().on_ground()
    }

    /// Returns true if the last movement was clipped horizontally.
    fn horizontal_collision(&self) -> bool {
        self.base().horizontal_collision()
    }

    /// Returns true if the last movement was clipped vertically.
    fn vertical_collision(&self) -> bool {
        self.base().vertical_collision()
    }

    /// Returns true if the last vertical collision was below the entity.
    fn vertical_collision_below(&self) -> bool {
        self.base().vertical_collision_below()
    }

    /// Returns true when movement bypasses collision physics.
    fn no_physics(&self) -> bool {
        self.base().no_physics()
    }

    /// Returns true when vanilla block-contact effects may run for this entity.
    fn is_affected_by_blocks(&self) -> bool {
        !self.is_removed() && !self.no_physics()
    }

    /// Sets whether this entity bypasses collision physics.
    fn set_no_physics(&self, no_physics: bool) {
        self.base().set_no_physics(no_physics);
    }

    /// Updates item-style `noPhysics` from the entity's current collision state.
    fn update_no_physics_from_current_collision(&self) {
        let Some(world) = self.level() else {
            self.set_no_physics(false);
            return;
        };

        let collision_world =
            WorldCollisionProvider::for_entity(&world, self.as_entity_event_source());
        // TODO: Include world-border collision once Steel has world-border physics.
        let colliding = collision_world.has_collision_with_context(
            &self.bounding_box().deflate(NO_PHYSICS_COLLISION_EPSILON),
            BlockCollisionContext::empty(),
        );
        self.set_no_physics(colliding);
        if colliding {
            let bounding_box = self.bounding_box();
            self.move_towards_closest_space(
                self.position().x,
                f64::midpoint(bounding_box.min_y(), bounding_box.max_y()),
                self.position().z,
            );
        }
    }

    /// Nudges velocity toward the closest non-full collision block.
    fn move_towards_closest_space(&self, x: f64, y: f64, z: f64) {
        let Some(world) = self.level() else {
            return;
        };

        let block_pos = BlockPos::containing(x, y, z);
        let fractional_position = DVec3::new(
            x - f64::from(block_pos.x()),
            y - f64::from(block_pos.y()),
            z - f64::from(block_pos.z()),
        );
        let closest_direction =
            closest_open_space_direction(block_pos, fractional_position, |neighbor_pos| {
                let block_state = world.get_block_state(neighbor_pos);
                let behavior = BLOCK_BEHAVIORS.get_behavior(block_state.get_block());
                let collision_shape = behavior.get_collision_shape(
                    block_state,
                    world.as_ref(),
                    neighbor_pos,
                    BlockCollisionContext::empty(),
                );
                is_shape_full_block(collision_shape)
            });

        let speed = {
            let mut random = self.base().random().lock();
            f64::from(random.next_f32().mul_add(0.2, 0.1))
        };
        let step = direction_step(closest_direction);
        let scaled_velocity = self.velocity() * 0.75;
        let next_velocity = match closest_direction.axis() {
            Axis::X => DVec3::new(step * speed, scaled_velocity.y, scaled_velocity.z),
            Axis::Y => DVec3::new(scaled_velocity.x, step * speed, scaled_velocity.z),
            Axis::Z => DVec3::new(scaled_velocity.x, scaled_velocity.y, step * speed),
        };
        self.set_velocity(next_velocity);
    }

    /// Default vanilla stuck-in-block movement for the next movement pass.
    fn default_make_stuck_in_block(&self, _state: BlockStateId, speed_multiplier: DVec3) {
        self.base().make_stuck_in_block(speed_multiplier);
    }

    /// Applies vanilla stuck-in-block movement for the next movement pass.
    fn make_stuck_in_block(&self, state: BlockStateId, speed_multiplier: DVec3) {
        self.default_make_stuck_in_block(state, speed_multiplier);
    }

    /// Applies current block-contact effects to this entity.
    ///
    /// Mirrors the shared ownership boundary of vanilla `Entity.applyEffectsFromBlocks`.
    fn apply_effects_from_blocks(&self) {
        let entity = self.as_entity_event_source();
        let movements = self.base().take_movements_for_block_effects();
        apply_effects_from_block_movements(entity, &movements);
    }

    /// Replays the last finalized block-contact movement list.
    fn apply_effects_from_blocks_for_last_movements(&self) {
        let entity = self.as_entity_event_source();
        let movements = self.base().last_movements_for_block_effects();
        apply_effects_from_block_movements(entity, &movements);
    }

    /// Sets whether the entity is on the ground.
    fn set_on_ground(&self, on_ground: bool) {
        let ground_contact = self.ground_contact_after_movement(on_ground, None);
        let movement_flags = self.base().movement_flags().with_on_ground(on_ground);
        self.base()
            .set_movement_flags(movement_flags, ground_contact);
    }

    /// Sets ground and horizontal collision flags from accepted movement.
    fn set_on_ground_with_movement(
        &self,
        on_ground: bool,
        horizontal_collision: bool,
        movement: DVec3,
    ) {
        let ground_contact = self.ground_contact_after_movement(on_ground, Some(movement));
        self.base()
            .set_on_ground_with_movement(on_ground, horizontal_collision, ground_contact);
    }

    /// Default final state application for accepted client-authored movement.
    ///
    /// Mirrors the shared tail of vanilla player and controlled-vehicle movement
    /// handling after rollback/collision validation has accepted the target.
    fn default_apply_accepted_client_movement(
        &self,
        world: &Arc<World>,
        accepted: AcceptedClientMovement,
    ) -> Result<AcceptedClientMovementOutcome, EntityMoveError> {
        if let Some(position) = accepted.position {
            self.try_set_position(position)?;
            self.refresh_fluid_contact();
        }

        self.set_rotation(accepted.rotation);
        self.set_on_ground_with_movement(
            accepted.on_ground,
            accepted.horizontal_collision,
            accepted.movement,
        );
        if self.do_check_fall_damage(accepted.movement, accepted.on_ground, world) {
            return Ok(AcceptedClientMovementOutcome::Handled);
        }
        if accepted.reset_fall_distance {
            self.reset_fall_distance();
        }

        Ok(AcceptedClientMovementOutcome::Applied)
    }

    /// Applies final state accepted from a client-authored movement packet.
    fn apply_accepted_client_movement(
        &self,
        world: &Arc<World>,
        accepted: AcceptedClientMovement,
    ) -> Result<AcceptedClientMovementOutcome, EntityMoveError> {
        self.default_apply_accepted_client_movement(world, accepted)
    }

    /// Applies final state accepted from a controlled-vehicle movement packet.
    fn apply_accepted_client_vehicle_movement(
        &self,
        world: &Arc<World>,
        mut accepted: AcceptedClientMovement,
    ) -> Result<AcceptedClientMovementOutcome, EntityMoveError> {
        accepted.horizontal_collision = self.horizontal_collision();
        accepted.reset_fall_distance = false;
        self.default_apply_accepted_client_movement(world, accepted)
    }

    /// Attempts to set the entity's position through world lifecycle validation.
    #[must_use = "movement commits can fail when world entity state rejects the update"]
    fn try_set_position(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        self.base().try_set_position(pos)
    }

    /// Sets the vanilla movement-trace old position to the current position.
    fn set_old_position_to_current(&self) {
        self.base().set_old_position_to_current();
    }

    /// Sets the vanilla movement-trace old position explicitly.
    fn set_old_position(&self, old_position: DVec3) {
        self.base().set_old_position(old_position);
    }

    /// Removes the latest movement segment recorded this tick.
    fn remove_latest_movement_recording(&self) {
        self.base().remove_latest_movement_recording();
    }

    /// Returns the block position this entity is standing on.
    fn on_pos(&self, offset: f32) -> Option<BlockPos> {
        let world = self.level()?;

        if let Some(supporting_block) = self.base().supporting_block() {
            if offset <= 1.0e-5 {
                return Some(supporting_block);
            }

            let below_state = world.get_block_state(supporting_block);
            let below_block = below_state.get_block();
            if (offset <= 0.5 && below_block.has_tag(&BlockTag::FENCES))
                || below_block.has_tag(&BlockTag::WALLS)
                || below_block.has_tag(&BlockTag::FENCE_GATES)
            {
                return Some(supporting_block);
            }

            return Some(BlockPos::new(
                supporting_block.x(),
                (self.position().y - f64::from(offset)).floor() as i32,
                supporting_block.z(),
            ));
        }

        let position = self.position();
        Some(BlockPos::new(
            position.x.floor() as i32,
            (position.y - f64::from(offset)).floor() as i32,
            position.z.floor() as i32,
        ))
    }

    /// Returns the block position used for movement-affecting block properties.
    fn block_pos_below_that_affects_movement(&self) -> Option<BlockPos> {
        self.on_pos(0.500_001)
    }

    /// Returns vanilla `getOnPosLegacy()`, used by fall/step block hooks.
    fn on_pos_legacy(&self) -> Option<BlockPos> {
        self.on_pos(0.2)
    }

    /// Returns the vanilla block speed factor applied after movement.
    #[expect(
        clippy::float_cmp,
        reason = "intentional: vanilla checks static block speed factors against 1.0"
    )]
    fn block_speed_factor(&self) -> f32 {
        let Some(world) = self.level() else {
            return 1.0;
        };

        let position = self.position();
        let current_state = world.get_block_state(BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        ));
        let current_block = current_state.get_block();
        let speed_factor_here = current_block.config.speed_factor;
        if current_block == &vanilla_blocks::WATER
            || current_block == &vanilla_blocks::BUBBLE_COLUMN
        {
            return speed_factor_here;
        }

        if speed_factor_here != 1.0 {
            return speed_factor_here;
        }

        let Some(below_pos) = self.block_pos_below_that_affects_movement() else {
            return 1.0;
        };
        world
            .get_block_state(below_pos)
            .get_block()
            .config
            .speed_factor
    }

    /// Returns vanilla `Entity.getBlockJumpFactor()`.
    #[expect(
        clippy::float_cmp,
        reason = "intentional: vanilla checks static block jump factors against 1.0"
    )]
    fn block_jump_factor(&self) -> f32 {
        let Some(world) = self.level() else {
            return 1.0;
        };

        let jump_factor_here = world
            .get_block_state(self.block_position())
            .get_block()
            .config
            .jump_factor;
        if jump_factor_here != 1.0 {
            return jump_factor_here;
        }

        let Some(below_pos) = self.block_pos_below_that_affects_movement() else {
            return 1.0;
        };
        world
            .get_block_state(below_pos)
            .get_block()
            .config
            .jump_factor
    }

    /// Returns this entity's physical pose.
    fn pose(&self) -> EntityPose {
        self.base().pose()
    }

    /// Returns whether vanilla currently considers this entity crouching.
    fn is_crouching(&self) -> bool {
        self.pose() == EntityPose::Sneaking
    }

    /// Returns whether vanilla currently considers this entity swimming.
    fn is_swimming(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_swimming)
    }

    /// Returns whether this entity is on rails.
    fn is_on_rails(&self) -> bool {
        false
    }

    /// Returns whether a block state is climbable for base movement effects.
    fn is_state_climbable(&self, state: BlockStateId) -> bool {
        let block = state.get_block();
        block.has_tag(&BlockTag::CLIMBABLE) || block == &vanilla_blocks::POWDER_SNOW
    }

    /// Returns vanilla movement side effects emitted by this entity.
    fn movement_emission(&self) -> EntityMovementEmission {
        EntityMovementEmission::All
    }

    /// Returns whether this entity may modify the world at a position.
    ///
    /// Vanilla `Entity.mayInteract` defaults to true; player-like entities can
    /// apply world permission checks through overrides.
    fn may_interact(&self, _world: &World, _pos: BlockPos) -> bool {
        true
    }

    /// Returns this entity's vanilla sound source category.
    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
    }

    /// Returns this entity's vanilla swim sound.
    fn swim_sound(&self) -> SoundEventRef {
        &sound_events::ENTITY_GENERIC_SWIM
    }

    /// Returns whether sounds from this entity are suppressed.
    fn is_silent(&self) -> bool {
        false
    }

    /// Broadcasts a vanilla entity event/status packet near this entity.
    fn broadcast_entity_event(&self, event: EntityStatus) {
        let Some(world) = self.level() else {
            return;
        };

        world.broadcast_to_nearby(
            ChunkPos::from_entity_pos(self.position()),
            CEntityEvent {
                entity_id: self.id(),
                event,
            },
            None,
        );
    }

    /// Plays an entity sound at the entity's exact position.
    fn play_sound(&self, sound: SoundEventRef, volume: f32, pitch: f32) {
        if self.is_silent() {
            return;
        }

        if let Some(world) = self.level() {
            world.play_sound_at(
                sound,
                self.sound_source(),
                self.position(),
                volume,
                pitch,
                None,
            );
        }
    }

    /// Plays vanilla's extinguished-on-fire entity sound.
    fn play_entity_on_fire_extinguished_sound(&self) {
        let pitch = {
            let mut random = self.base().random().lock();
            1.6 + (random.next_f32() - random.next_f32()) * 0.4
        };
        self.play_sound(&sound_events::ENTITY_GENERIC_EXTINGUISH_FIRE, 0.7, pitch);
    }

    /// Plays the base vanilla step sound for a block.
    fn play_step_sound(&self, _pos: BlockPos, block_state: BlockStateId) {
        self.play_block_step_sound(block_state);
    }

    /// Plays a vanilla block step sound at this entity's current position.
    fn play_block_step_sound(&self, block_state: BlockStateId) {
        let sound_type = block_state.get_block().config.sound_type;
        self.play_sound(
            sound_type.step_sound,
            sound_type.volume * 0.15,
            sound_type.pitch,
        );
    }

    /// Plays vanilla's muffled secondary step sound.
    fn play_muffled_step_sound(&self, block_state: BlockStateId) {
        let sound_type = block_state.get_block().config.sound_type;
        self.play_sound(
            sound_type.step_sound,
            sound_type.volume * 0.05,
            sound_type.pitch * 0.8,
        );
    }

    /// Plays vanilla's combination primary and secondary step sounds.
    fn play_combination_step_sounds(
        &self,
        primary_step_sound: BlockStateId,
        secondary_step_sound: BlockStateId,
    ) {
        self.play_block_step_sound(primary_step_sound);
        self.play_muffled_step_sound(secondary_step_sound);
    }

    /// Plays vanilla walking step sounds, including amethyst chimes.
    fn walking_step_sound(&self, pos: BlockPos, block_state: BlockStateId) {
        self.play_step_sound(pos, block_state);
        if block_state
            .get_block()
            .has_tag(&BlockTag::CRYSTAL_SOUND_BLOCKS)
        {
            self.play_amethyst_step_sound();
        }
    }

    /// Plays vanilla amethyst step chime when its cooldown permits it.
    fn play_amethyst_step_sound(&self) {
        let Some(sound) = self.base().amethyst_step_sound(self.tick_count()) else {
            return;
        };
        self.play_sound(
            &sound_events::BLOCK_AMETHYST_BLOCK_CHIME,
            sound.volume,
            sound.pitch,
        );
    }

    /// Plays vanilla swim sound from movement emission.
    fn water_swim_sound(&self) {
        let velocity = self.velocity();
        let volume = ((velocity.x * velocity.x * 0.2
            + velocity.y * velocity.y
            + velocity.z * velocity.z * 0.2)
            .sqrt() as f32
            * 0.35)
            .min(1.0);
        self.play_swim_sound(volume);
    }

    /// Plays this entity's swim sound at the given volume.
    fn play_swim_sound(&self, volume: f32) {
        let pitch = {
            let mut random = self.base().random().lock();
            1.0 + (random.next_f32() - random.next_f32()) * 0.4
        };
        self.play_sound(self.swim_sound(), volume, pitch);
    }

    /// Returns whether the entity is currently flapping.
    fn is_flapping(&self) -> bool {
        false
    }

    /// Runs entity-specific flap side effects.
    fn on_flap(&self) {}

    /// Processes vanilla flap movement side effects.
    fn process_flapping_movement(&self) {
        if !self.is_flapping() {
            return;
        }

        self.on_flap();
        if self.movement_emission().emits_events()
            && let Some(world) = self.level()
        {
            world.game_event_at(
                &vanilla_game_events::FLAP,
                self.position(),
                &GameEventContext::new(Some(self.as_entity_event_source()), None),
            );
        }
    }

    /// Returns the next step threshold after movement side effects are produced.
    fn next_step(&self) -> f32 {
        self.base().movement_progress().move_dist().floor() + 1.0
    }

    /// Applies vanilla movement sounds and game events after a completed move.
    fn apply_movement_emission_and_play_sound(
        &self,
        emission: EntityMovementEmission,
        clipped_movement: DVec3,
        effect_pos: BlockPos,
        effect_state: BlockStateId,
    ) {
        let Some(world) = self.level() else {
            return;
        };
        let Some(supporting_pos) = self.on_pos(1.0e-5) else {
            return;
        };

        let supporting_state = world.get_block_state(supporting_pos);
        let climbing = self.is_state_climbable(supporting_state);
        let progress = self
            .base()
            .record_movement_progress(clipped_movement, climbing);

        if progress.crossed_next_step() && supporting_state.get_block() != &vanilla_blocks::AIR {
            let only_effect_state_emissions = supporting_pos == effect_pos;
            let mut produced_side_effects = self.vibration_and_sound_effects_from_block(
                effect_pos,
                effect_state,
                emission.emits_sounds(),
                only_effect_state_emissions,
                clipped_movement,
            );
            if !only_effect_state_emissions {
                produced_side_effects |= self.vibration_and_sound_effects_from_block(
                    supporting_pos,
                    supporting_state,
                    false,
                    emission.emits_events(),
                    clipped_movement,
                );
            }

            if produced_side_effects {
                self.base().set_next_step(self.next_step());
            } else if self.is_in_water() {
                self.base().set_next_step(self.next_step());
                if emission.emits_sounds() {
                    self.water_swim_sound();
                }
                if emission.emits_events() {
                    world.game_event_at(
                        &vanilla_game_events::SWIM,
                        self.position(),
                        &GameEventContext::new(Some(self.as_entity_event_source()), None),
                    );
                }
            }
        } else if supporting_state.get_block() == &vanilla_blocks::AIR {
            self.process_flapping_movement();
        }
    }

    /// Applies movement side effects after vanilla collision and landing updates.
    fn apply_movement_side_effects_after_move(&self, world: &World, actual_movement: DVec3) {
        let emission = self.movement_emission();
        if !emission.emits_anything() || self.is_passenger() {
            return;
        }

        let Some(effect_pos) = self.on_pos_legacy() else {
            return;
        };
        let effect_state = world.get_block_state(effect_pos);
        self.apply_movement_emission_and_play_sound(
            emission,
            actual_movement,
            effect_pos,
            effect_state,
        );
    }

    /// Emits step side effects from a candidate movement-effect block.
    fn vibration_and_sound_effects_from_block(
        &self,
        pos: BlockPos,
        block_state: BlockStateId,
        should_sound: bool,
        should_vibrate: bool,
        clipped_movement: DVec3,
    ) -> bool {
        if block_state.get_block() == &vanilla_blocks::AIR {
            return false;
        }

        let is_climbable = self.is_state_climbable(block_state);
        if !(self.on_ground()
            || is_climbable
            || self.is_crouching() && clipped_movement.y == 0.0
            || self.is_on_rails())
            || self.is_swimming()
        {
            return false;
        }

        if should_sound {
            self.walking_step_sound(pos, block_state);
        }
        if should_vibrate && let Some(world) = self.level() {
            world.game_event_at(
                &vanilla_game_events::STEP,
                self.position(),
                &GameEventContext::new(Some(self.as_entity_event_source()), Some(block_state)),
            );
        }

        true
    }

    /// Maximum height this entity can step up during normal movement.
    fn max_up_step(&self) -> f32 {
        0.0
    }

    /// Whether movement should apply player-style sneak edge prevention.
    fn backs_off_from_edge(&self) -> bool {
        false
    }

    // These mirror vanilla's Entity class methods.

    /// Gets the default gravity for this entity type.
    ///
    /// Override this to specify entity-specific gravity.
    /// Vanilla values: `ItemEntity` = 0.04, `LivingEntity` = 0.08
    fn get_default_gravity(&self) -> f64 {
        0.0
    }

    /// Returns true if gravity is disabled for this entity.
    fn is_no_gravity(&self) -> bool {
        self.synced_data()
            .map_or_else(|| self.base().no_gravity(), EntitySyncedData::is_no_gravity)
    }

    /// Sets the shared vanilla `NoGravity` flag.
    fn set_no_gravity(&self, no_gravity: bool) {
        self.base().set_no_gravity(no_gravity);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_no_gravity(no_gravity);
        }
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

    /// Applies vanilla `Entity.moveRelative()`.
    fn move_relative(&self, speed: f32, input: DVec3) {
        let yaw = self.rotation().0;
        self.set_velocity(self.velocity() + get_input_vector(input, speed, yaw));
    }

    /// Moves the entity without collision physics.
    fn move_without_physics(&self, delta: DVec3) -> Option<MoveResult> {
        let final_position = self.position() + delta;
        if let Err(error) = self.try_set_position(final_position) {
            log::debug!(
                "Rejected no-physics movement for entity {}: {error}",
                self.id()
            );
            return None;
        }
        self.base().clear_collision_flags();
        self.refresh_fluid_contact();

        Some(MoveResult {
            final_position,
            actual_movement: delta,
            on_ground: self.on_ground(),
            horizontal_collision: false,
            vertical_collision: false,
            x_collision: false,
            z_collision: false,
            final_aabb: self.bounding_box(),
        })
    }

    /// Moves the entity with collision detection.
    ///
    /// Mirrors vanilla's `Entity.move(MoverType, Vec3)`.
    /// Updates position, `on_ground`, velocity (on collision), and returns collision info.
    fn move_entity(&self, mover_type: MoverType, delta: DVec3) -> Option<MoveResult> {
        let world = self.level()?;
        if self.no_physics() {
            return self.move_without_physics(delta);
        }

        let mut movement = delta;
        if mover_type == MoverType::Piston {
            let game_time = world.level_data.read().game_time();
            movement = self.base().limit_piston_movement(movement, game_time);
            if movement == DVec3::ZERO {
                return None;
            }
        }
        movement = self
            .base()
            .consume_stuck_speed_multiplier(movement, mover_type != MoverType::Piston);

        let physics_state = physics_state_for_move(self.as_entity_event_source());
        let start_position = physics_state.position();

        // Perform collision detection and movement
        let collision_world =
            WorldCollisionProvider::for_entity(&world, self.as_entity_event_source());
        let result =
            resolve_entity_movement(&physics_state, movement, mover_type, &collision_world);

        record_movement_for_block_effects(
            self.as_entity_event_source(),
            start_position,
            result.final_position,
            movement,
            result.actual_movement,
        );

        // Update entity state
        if should_apply_resolved_movement(movement, result.actual_movement) {
            self.reset_fall_distance_on_resetting_clip(&world, result.actual_movement);
            if let Err(error) = self.try_set_position(result.final_position) {
                log::debug!(
                    "Rejected resolved movement for entity {}: {error}",
                    self.id()
                );
                self.remove_latest_movement_recording();
                return None;
            }
        }
        let vertical_state_update =
            EntityVerticalMovementStateUpdate::for_move(movement, self.is_server_driven_movement());
        let movement_flags = EntityMovementFlags::after_move_with_previous(
            self.base().movement_flags(),
            vertical_state_update,
            result.on_ground,
            result.horizontal_collision,
            result.vertical_collision,
            movement,
        );
        let ground_contact = if vertical_state_update.refreshes_state() {
            self.ground_contact_after_movement(result.on_ground, Some(result.actual_movement))
        } else {
            self.base().ground_contact()
        };
        self.base()
            .set_movement_flags(movement_flags, ground_contact);
        self.refresh_fluid_contact();

        if self.is_server_driven_movement() && self.apply_fall_damage_after_move(&result, &world) {
            return Some(result);
        }

        // Vanilla: Entity.move() zeros velocity components on collision.
        // Horizontal collision zeros X/Z individually based on which axis collided.
        // Vertical collision calls Block.updateEntityMovementAfterFallOn.
        // The default block behavior zeros Y velocity; block-specific behavior
        // can override this for slime, beds, and similar landing surfaces.
        if result.horizontal_collision {
            let vel = self.velocity();
            self.set_velocity(DVec3::new(
                if result.x_collision { 0.0 } else { vel.x },
                vel.y,
                if result.z_collision { 0.0 } else { vel.z },
            ));
        }
        if result.vertical_collision && self.can_simulate_movement() {
            let velocity = self.velocity();
            let landing_context = EntityLandingContext::new(
                velocity,
                self.is_living_entity(),
                self.is_suppressing_bounce(),
            );
            let next_velocity =
                if let Some(effect_pos) = self.block_pos_below_that_affects_movement() {
                    let effect_state = world.get_block_state(effect_pos);
                    let behavior = BLOCK_BEHAVIORS.get_behavior(effect_state.get_block());
                    behavior.update_entity_movement_after_fall_on(
                        effect_state,
                        &world,
                        effect_pos,
                        landing_context,
                    )
                } else {
                    landing_context.default_velocity_after_fall_on()
                };
            self.set_velocity(next_velocity);
        }

        self.apply_movement_side_effects_after_move(&world, result.actual_movement);

        let speed_factor = f64::from(self.block_speed_factor());
        let vel = self.velocity();
        self.set_velocity(DVec3::new(
            vel.x * speed_factor,
            vel.y,
            vel.z * speed_factor,
        ));

        Some(result)
    }

    /// Applies vanilla fall-distance bookkeeping after accepted movement.
    fn apply_fall_damage_after_move(&self, result: &MoveResult, world: &Arc<World>) -> bool {
        self.do_check_fall_damage(result.actual_movement, result.on_ground, world)
    }

    /// Resets fall distance when vanilla's fall-damage-resetting clip hits.
    fn reset_fall_distance_on_resetting_clip(&self, world: &Arc<World>, movement: DVec3) {
        let Some(check_to) =
            fall_damage_reset_clip_target(self.position(), movement, self.fall_distance())
        else {
            return;
        };

        let hit = world.clip(
            self.position(),
            check_to,
            ClipBlockShape::FallDamageResetting {
                entity_is_player: self.entity_type() == &vanilla_entities::PLAYER,
            },
            ClipFluid::Water,
        );
        if !hit.is_miss() {
            self.reset_fall_distance();
        }
    }

    /// Mirrors vanilla `Entity.doCheckFallDamage`.
    ///
    /// Callers update on-ground/supporting-block state before this method.
    fn do_check_fall_damage(&self, movement: DVec3, on_ground: bool, world: &Arc<World>) -> bool {
        let Some(effect_pos) = self.on_pos_legacy() else {
            return false;
        };
        let effect_state = world.get_block_state(effect_pos);
        self.check_fall_damage(movement.y, on_ground, effect_state, effect_pos, world);
        self.is_removed()
    }

    /// Refreshes vanilla supporting-block state before fall-damage side effects.
    fn refresh_supporting_block_for_fall_damage(&self, movement: DVec3, on_ground: bool) {
        let ground_contact = self.ground_contact_after_movement(on_ground, Some(movement));
        self.base().set_ground_contact(ground_contact);
    }

    /// Mirrors vanilla `Entity.checkFallDamage`.
    fn check_fall_damage(
        &self,
        vertical_movement: f64,
        on_ground: bool,
        on_state: BlockStateId,
        pos: BlockPos,
        world: &Arc<World>,
    ) {
        if !self.is_in_water() && vertical_movement < 0.0 {
            self.base().accumulate_fall_distance(vertical_movement);
        }

        if !on_ground {
            return;
        }

        let fall_distance = self.fall_distance();
        if fall_distance > 0.0 {
            let behavior = BLOCK_BEHAVIORS.get_behavior(on_state.get_block());
            let fall_context =
                EntityFallOnContext::from_entity(fall_distance, self.as_entity_event_source());
            if let Some(fall_damage) = behavior.fall_on(on_state, world, pos, fall_context) {
                let damage_applied = self.cause_fall_damage(
                    fall_damage.fall_distance,
                    fall_damage.damage_modifier,
                    &fall_damage.source,
                );
                behavior.after_fall_on_damage(
                    on_state,
                    world,
                    pos,
                    self.as_entity_event_source(),
                    &fall_damage,
                    damage_applied,
                );
            }

            let supporting_state = self
                .base()
                .supporting_block()
                .map_or(on_state, |supporting_pos| {
                    world.get_block_state(supporting_pos)
                });
            world.game_event(
                &vanilla_game_events::HIT_GROUND,
                BlockPos::new(
                    self.position().x.floor() as i32,
                    self.position().y.floor() as i32,
                    self.position().z.floor() as i32,
                ),
                &GameEventContext::new(Some(self.as_entity_event_source()), Some(supporting_state)),
            );
        }

        self.reset_fall_distance();
    }

    /// Computes vanilla support state for an on-ground update.
    fn ground_contact_after_movement(
        &self,
        on_ground: bool,
        movement: Option<DVec3>,
    ) -> EntityGroundContact {
        let Some(world) = self.level() else {
            return if on_ground {
                EntityGroundContact::on_ground(None)
            } else {
                EntityGroundContact::airborne()
            };
        };

        self.check_supporting_block(on_ground, movement, &world)
    }

    /// Mirrors vanilla `Entity.checkSupportingBlock`.
    fn check_supporting_block(
        &self,
        on_ground: bool,
        movement: Option<DVec3>,
        world: &Arc<World>,
    ) -> EntityGroundContact {
        if !on_ground {
            return EntityGroundContact::airborne();
        }

        let bounding_box = self.bounding_box();
        let test_area = WorldAabb::new(
            bounding_box.min_x(),
            bounding_box.min_y() - 1.0e-6,
            bounding_box.min_z(),
            bounding_box.max_x(),
            bounding_box.min_y(),
            bounding_box.max_z(),
        );
        let collision_world =
            WorldCollisionProvider::for_entity(world, self.as_entity_event_source());
        let descending = self.is_descending();
        let mut supporting_block =
            collision_world.find_supporting_block(self.position(), &test_area, descending);

        if supporting_block.is_none()
            && !self.base().on_ground_no_blocks()
            && let Some(movement) = movement
        {
            let previous_test_area = test_area.move_by(-movement.x, 0.0, -movement.z);
            supporting_block = collision_world.find_supporting_block(
                self.position(),
                &previous_test_area,
                descending,
            );
        }

        EntityGroundContact::on_ground(supporting_block)
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
        world.spawn_item(DVec3::new(pos.x, pos.y + y_offset, pos.z), item)
    }

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

    /// Applies damage to this entity.
    ///
    /// Vanilla: `Entity.hurtServer()` — overridden by `LivingEntity` (complex
    /// armor/effects/invulnerability logic) and `ItemEntity` (health decrement
    /// and discard). Default returns `false` (entity ignores damage).
    #[expect(
        unused_variables,
        reason = "default trait impl; parameters used by overrides"
    )]
    fn hurt(&self, source: &DamageSource, amount: f32) -> bool {
        false
    }

    /// Teleports an entity from one loaded world to another.
    ///
    /// The default implementation logs a warning — non-player entity teleportation
    /// is not yet implemented. Override in entity types that support it.
    fn change_world(self: Arc<Self>, _teleport_transition: &TeleportTransition) {
        log::warn!(
            "change_world called on entity {} which does not implement world changes",
            self.id(),
        );
    }
}

/// A trait for living entities that can take damage, heal, and die.
///
/// This trait provides the core functionality for entities that have health,
/// can be damaged, and can die. It's based on Minecraft's `LivingEntity` class.
///
/// **Note:** All methods take `&self` (not `&mut self`) because living entities
/// are shared via `Arc` and use interior mutability (`SyncMutex`, etc.).
pub trait LivingEntity: Entity {
    /// Returns a reference to the shared [`LivingEntityBase`] that holds
    /// living runtime state such as attributes, cached movement speed,
    /// damage cooldown, and death animation counters.
    fn living_base(&self) -> &LivingEntityBase;

    /// Returns a reference to this entity's attribute map.
    fn attributes(&self) -> &SyncMutex<AttributeMap> {
        self.living_base().attributes()
    }

    /// Gets the current health of the entity.
    fn get_health(&self) -> f32;

    /// Sets the health of the entity, clamped between 0 and max health.
    fn set_health(&self, health: f32);

    /// Gets the maximum health from the attribute system.
    fn get_max_health(&self) -> f32 {
        self.attributes()
            .lock()
            .required_value(vanilla_attributes::MAX_HEALTH) as f32
    }

    /// Heals the entity by the specified amount.
    fn heal(&self, amount: f32) {
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

    /// Gets the absorption amount (extra health from effects like absorption).
    fn get_absorption_amount(&self) -> f32;

    /// Sets the absorption amount.
    fn set_absorption_amount(&self, amount: f32);

    /// Returns vanilla `LivingEntity.getFallDamageSound()`.
    fn fall_damage_sound(&self, damage: i32) -> SoundEventRef {
        let (small, big) = self.fall_sounds();
        if damage > 4 { big } else { small }
    }

    /// Gets the entity's armor value from the attribute system.
    fn get_armor_value(&self) -> i32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::ARMOR)
            .unwrap_or(0.0) as i32
    }

    /// Gets the gravity value from the attribute system.
    fn get_attribute_gravity(&self) -> f64 {
        self.attributes()
            .lock()
            .required_value(vanilla_attributes::GRAVITY)
    }

    /// Returns vanilla `LivingEntity.getEffectiveGravity()`.
    fn get_effective_gravity(&self) -> f64 {
        let gravity = self.get_gravity();
        if self.velocity().y <= 0.0 && self.has_mob_effect(vanilla_mob_effects::SLOW_FALLING) {
            gravity.min(0.01)
        } else {
            gravity
        }
    }

    /// Checks if the entity can be affected by potions.
    fn is_affected_by_potions(&self) -> bool {
        true
    }

    /// Returns vanilla `LivingEntity.hasEffect()`.
    fn has_mob_effect(&self, effect: MobEffectRef) -> bool {
        self.living_base().has_mob_effect(effect)
    }

    /// Returns vanilla `LivingEntity.getEffect()`.
    fn mob_effect(&self, effect: MobEffectRef) -> Option<ActiveMobEffect> {
        self.living_base().mob_effect(effect)
    }

    /// Sets active vanilla mob-effect state.
    fn set_mob_effect(&self, effect: MobEffectRef, amplifier: i32) {
        self.living_base().set_mob_effect(effect, amplifier);
    }

    /// Sets the presence of a vanilla mob effect.
    fn set_mob_effect_active(&self, effect: MobEffectRef, active: bool) {
        self.living_base().set_mob_effect_active(effect, active);
    }

    /// Returns vanilla `LivingEntity.isAffectedByFluids()`.
    fn is_affected_by_fluids(&self) -> bool {
        true
    }

    /// Returns vanilla `LivingEntity.canStandOnFluid()`.
    fn can_stand_on_fluid(&self, _fluid_state: FluidState) -> bool {
        false
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
        self.living_base().is_fall_flying()
    }

    /// Sets whether this entity is fall flying.
    fn set_fall_flying(&self, fall_flying: bool) {
        self.set_shared_fall_flying(fall_flying);
        self.living_base().set_fall_flying(fall_flying);
    }

    /// Returns vanilla `LivingEntity.getFallFlyingTicks()`.
    fn fall_flying_ticks(&self) -> i32 {
        self.living_base().fall_flying_ticks()
    }

    /// Visits the item in a vanilla living-entity equipment slot.
    fn with_equipment_slot(&self, slot: EquipmentSlot, visitor: &mut dyn FnMut(&ItemStack)) {
        let equipment = self.living_base().equipment().lock();
        visitor(equipment.get_ref(slot));
    }

    /// Mutates the item in a vanilla living-entity equipment slot.
    fn with_equipment_slot_mut(
        &self,
        slot: EquipmentSlot,
        visitor: &mut dyn FnMut(&mut ItemStack),
    ) {
        let mut equipment = self.living_base().equipment().lock();
        visitor(equipment.get_mut(slot));
    }

    /// Returns whether equipment durability should be skipped for this entity.
    fn has_infinite_materials(&self) -> bool {
        false
    }

    /// Called after an equipped item breaks.
    fn on_equipped_item_broken(&self, _slot: EquipmentSlot) {
        // TODO: Broadcast vanilla equipped-item break events once item break callbacks exist.
    }

    /// Returns vanilla `LivingEntity.canFreeze()` after concrete entity exemptions.
    ///
    /// Vanilla keeps the entity-type freeze immunity on `Entity` and the equipment
    /// immunity on `LivingEntity`. Steel keeps this helper separate so concrete
    /// `Entity::can_freeze` implementations can delegate without downcasting.
    fn default_living_can_freeze(&self) -> bool {
        for slot in EquipmentSlot::ARMOR_SLOTS {
            let mut is_freeze_immune = false;
            self.with_equipment_slot(slot, &mut |item_stack| {
                is_freeze_immune = REGISTRY
                    .items
                    .is_in_tag(item_stack.item(), &ItemTag::FREEZE_IMMUNE_WEARABLES);
            });

            if is_freeze_immune {
                return false;
            }
        }

        self.default_can_freeze()
    }

    /// Returns vanilla `PowderSnowBlock.canEntityWalkOnPowderSnow()` for living entities.
    fn default_living_can_walk_on_powder_snow(&self) -> bool {
        if self.default_can_walk_on_powder_snow() {
            return true;
        }

        let mut has_leather_boots = false;
        self.with_equipment_slot(EquipmentSlot::Feet, &mut |item_stack| {
            has_leather_boots = item_stack.is(&vanilla_items::ITEMS.leather_boots);
        });
        has_leather_boots
    }

    /// Ticks living-entity counters after movement.
    fn tick_living_state(&self) {
        self.living_base()
            .tick_fall_flying_state(self.is_fall_flying());
        self.living_base().tick_post_impulse_grace_time();
    }

    /// Mirrors vanilla `LivingEntity.canGlideUsing()`.
    fn can_glide_using(&self, item_stack: &ItemStack, slot: EquipmentSlot) -> bool {
        let Some(equippable) = item_stack.get_equippable() else {
            return false;
        };

        item_stack.has(GLIDER)
            && equipment_slot_matches_equippable(slot, equippable.slot)
            && !item_stack.next_damage_will_break()
    }

    /// Returns whether the item in `slot` can be used for vanilla gliding.
    fn can_glide_using_equipment_slot(&self, slot: EquipmentSlot) -> bool {
        let mut can_glide = false;
        self.with_equipment_slot(slot, &mut |item_stack| {
            can_glide = self.can_glide_using(item_stack, slot);
        });
        can_glide
    }

    /// Damages one random equipped glider like vanilla `LivingEntity.updateFallFlying()`.
    fn damage_random_glider(&self) {
        let mut slots_with_gliders = Vec::new();
        for slot in EquipmentSlot::ALL {
            if self.can_glide_using_equipment_slot(slot) {
                slots_with_gliders.push(slot);
            }
        }

        let slot_count = slots_with_gliders.len();
        if slot_count == 0 {
            return;
        }

        let slot_index = self
            .base()
            .random()
            .lock()
            .next_i32_bounded(slot_count as i32) as usize;
        let slot_to_damage = slots_with_gliders[slot_index];
        let has_infinite_materials = self.has_infinite_materials();
        let mut item_broke = false;
        self.with_equipment_slot_mut(slot_to_damage, &mut |item_stack| {
            item_broke = item_stack.hurt_and_break(1, has_infinite_materials);
        });
        if item_broke {
            self.on_equipped_item_broken(slot_to_damage);
        }
    }

    /// Default vanilla `LivingEntity.canGlide()` implementation for overrides.
    fn default_can_glide(&self) -> bool {
        !self.on_ground()
            && !self.is_passenger()
            && !self.has_mob_effect(vanilla_mob_effects::LEVITATION)
            && EquipmentSlot::ALL
                .iter()
                .any(|&slot| self.can_glide_using_equipment_slot(slot))
    }

    /// Mirrors vanilla `LivingEntity.canGlide()`.
    fn can_glide(&self) -> bool {
        self.default_can_glide()
    }

    /// Mirrors vanilla `Player.startFallFlying()`.
    fn start_fall_flying(&self) {
        self.set_fall_flying(true);
    }

    /// Mirrors vanilla `Player.tryToStartFallFlying()`.
    fn try_to_start_fall_flying(&self) -> bool {
        if !self.is_fall_flying() && self.can_glide() && !self.is_in_water() {
            self.start_fall_flying();
            return true;
        }

        false
    }

    /// Returns the last climbable block position this living entity touched.
    fn last_climbable_pos(&self) -> Option<BlockPos> {
        self.living_base().last_climbable_pos()
    }

    /// Records the last climbable block position this living entity touched.
    fn set_last_climbable_pos(&self, pos: BlockPos) {
        self.living_base().set_last_climbable_pos(pos);
    }

    /// Returns vanilla `LivingEntity.onClimbable()` behavior.
    fn default_living_on_climbable(&self) -> bool {
        if self.is_spectator() {
            return false;
        }

        let pos = self.block_position();
        let Some(world) = self.level() else {
            return false;
        };
        let state = world.get_block_state(pos);
        let block = state.get_block();

        if self.is_fall_flying() && block.has_tag(&BlockTag::CAN_GLIDE_THROUGH) {
            return false;
        }

        let climbable = block.has_tag(&BlockTag::CLIMBABLE)
            || block.has_tag(&BlockTag::TRAPDOORS)
                && trapdoor_usable_as_ladder_state(state, world.get_block_state(pos.below()));

        if climbable {
            self.set_last_climbable_pos(pos);
        }

        climbable
    }

    /// Returns whether vanilla living travel should skip friction damping.
    fn should_discard_friction(&self) -> bool {
        self.living_base().should_discard_friction()
    }

    /// Sets whether vanilla living travel should skip friction damping.
    fn set_discard_friction(&self, discard_friction: bool) {
        self.living_base().set_discard_friction(discard_friction);
    }

    /// Returns whether this living entity is currently applying jump input.
    fn is_jumping(&self) -> bool {
        self.living_base().is_jumping()
    }

    /// Sets whether this living entity is currently applying jump input.
    fn set_jumping(&self, jumping: bool) {
        self.living_base().set_jumping(jumping);
    }

    /// Returns vanilla living travel input.
    fn travel_input(&self) -> LivingTravelInput {
        self.living_base().travel_input()
    }

    /// Sets vanilla living travel input.
    fn set_travel_input(&self, input: LivingTravelInput) {
        self.living_base().set_travel_input(input);
    }

    /// Applies vanilla `LivingEntity.applyInput()` damping.
    fn apply_input(&self) {
        self.living_base().dampen_travel_input();
    }

    /// Returns vanilla jump cooldown ticks.
    fn no_jump_delay(&self) -> i32 {
        self.living_base().no_jump_delay()
    }

    /// Sets vanilla jump cooldown ticks.
    fn set_no_jump_delay(&self, ticks: i32) {
        self.living_base().set_no_jump_delay(ticks);
    }

    /// Decrements vanilla jump cooldown once per living AI step.
    fn tick_no_jump_delay(&self) {
        self.living_base().tick_no_jump_delay();
    }

    /// Returns vanilla `LivingEntity.isImmobile()`.
    fn default_is_immobile(&self) -> bool {
        self.is_dead_or_dying()
    }

    /// Returns vanilla `LivingEntity.isImmobile()`.
    fn is_immobile(&self) -> bool {
        self.default_is_immobile()
    }

    /// Applies vanilla `LivingEntity.aiStep()` velocity thresholds.
    fn apply_living_velocity_thresholds(&self) {
        let movement = self.velocity();
        let mut dx = movement.x;
        let mut dy = movement.y;
        let mut dz = movement.z;

        if self.entity_type() == &vanilla_entities::PLAYER {
            if movement.x.mul_add(movement.x, movement.z * movement.z) < 9.0E-6 {
                dx = 0.0;
                dz = 0.0;
            }
        } else {
            if movement.x.abs() < 0.003 {
                dx = 0.0;
            }
            if movement.z.abs() < 0.003 {
                dz = 0.0;
            }
        }

        if movement.y.abs() < 0.003 {
            dy = 0.0;
        }

        self.set_velocity(DVec3::new(dx, dy, dz));
    }

    /// Server AI hook called from vanilla `LivingEntity.aiStep()`.
    fn server_ai_step(&self) {}

    /// Returns vanilla `LivingEntity.getJumpBoostPower()`.
    fn get_jump_boost_power(&self) -> f32 {
        self.mob_effect(vanilla_mob_effects::JUMP_BOOST)
            .map_or(0.0, |effect| 0.1 * (effect.amplifier() as f32 + 1.0))
    }

    /// Returns vanilla `LivingEntity.getJumpPower(float)`.
    fn get_jump_power_with_multiplier(&self, multiplier: f32) -> f32 {
        let jump_strength =
            self.attributes()
                .lock()
                .get_value(vanilla_attributes::JUMP_STRENGTH)
                .unwrap_or(vanilla_attributes::JUMP_STRENGTH.default_value) as f32;
        jump_strength * multiplier * self.block_jump_factor() + self.get_jump_boost_power()
    }

    /// Returns vanilla `LivingEntity.getJumpPower()`.
    fn get_jump_power(&self) -> f32 {
        self.get_jump_power_with_multiplier(1.0)
    }

    /// Default vanilla `LivingEntity.jumpFromGround()` implementation for overrides.
    fn default_jump_from_ground(&self) {
        let jump_power = self.get_jump_power();
        if jump_power <= 1.0E-5 {
            return;
        }

        let movement = self.velocity();
        self.set_velocity(DVec3::new(
            movement.x,
            movement.y.max(f64::from(jump_power)),
            movement.z,
        ));
        if self.is_sprinting() {
            let angle = self.rotation().0.to_radians();
            self.set_velocity(
                self.velocity()
                    + DVec3::new(
                        f64::from(-angle.sin() * 0.2),
                        0.0,
                        f64::from(angle.cos() * 0.2),
                    ),
            );
        }

        self.mark_velocity_sync();
    }

    /// Mirrors vanilla `LivingEntity.jumpFromGround()`.
    fn jump_from_ground(&self) {
        self.default_jump_from_ground();
    }

    /// Mirrors vanilla `LivingEntity.goDownInWater()`.
    fn go_down_in_water(&self) {
        self.set_velocity(self.velocity() + DVec3::new(0.0, f64::from(-0.04_f32), 0.0));
    }

    /// Mirrors vanilla `LivingEntity.jumpInLiquid()`.
    fn jump_in_liquid(&self, _fluid_tag: &Identifier) {
        self.set_velocity(self.velocity() + DVec3::new(0.0, f64::from(0.04_f32), 0.0));
    }

    /// Applies vanilla `LivingEntity.aiStep()` jump handling.
    fn handle_living_jump(&self) {
        if !self.is_jumping() || !self.is_affected_by_fluids() {
            self.set_no_jump_delay(0);
            return;
        }

        let fluid_height = if self.is_in_lava() {
            self.fluid_contact().lava_height()
        } else {
            self.fluid_contact().water_height()
        };
        let in_water_and_has_fluid_height = self.is_in_water() && fluid_height > 0.0;
        let fluid_jump_threshold = self.get_fluid_jump_threshold();
        if !in_water_and_has_fluid_height
            || self.on_ground() && fluid_height <= fluid_jump_threshold
        {
            if !self.is_in_lava() || self.on_ground() && fluid_height <= fluid_jump_threshold {
                if (self.on_ground()
                    || in_water_and_has_fluid_height && fluid_height <= fluid_jump_threshold)
                    && self.no_jump_delay() == 0
                {
                    self.jump_from_ground();
                    self.set_no_jump_delay(10);
                }
            } else {
                self.jump_in_liquid(&vanilla_fluid_tags::FluidTag::LAVA);
            }
        } else {
            self.jump_in_liquid(&vanilla_fluid_tags::FluidTag::WATER);
        }
    }

    /// Default vanilla-shaped `LivingEntity.aiStep()` movement foundation for overrides.
    ///
    /// This covers the shared travel state Steel currently has; mob AI and
    /// equipment ticking are still separate follow-up work.
    fn default_ai_step(&self) -> Option<MoveResult> {
        self.tick_no_jump_delay();
        if !self.can_simulate_movement() {
            self.set_velocity(self.velocity() * 0.98);
        }

        self.apply_living_velocity_thresholds();
        self.apply_input();
        if self.is_immobile() {
            self.set_jumping(false);
            let input = self.travel_input();
            self.set_travel_input(LivingTravelInput::new(0.0, input.vertical(), 0.0));
        } else if self.is_effective_ai() {
            self.server_ai_step();
        }

        self.handle_living_jump();

        if !self.can_simulate_movement() || !self.is_effective_ai() {
            return None;
        }

        if self.is_fall_flying() {
            self.update_fall_flying();
        }

        let input = self.travel_input();
        self.travel(DVec3::new(
            f64::from(input.sideways()),
            f64::from(input.vertical()),
            f64::from(input.forward()),
        ))
    }

    /// Mirrors vanilla `LivingEntity.aiStep()`.
    fn ai_step(&self) -> Option<MoveResult> {
        self.default_ai_step()
    }

    /// Returns vanilla `LivingEntity.isSuppressingSlidingDownLadder()`.
    fn is_suppressing_sliding_down_ladder(&self) -> bool {
        self.is_suppressing_bounce()
    }

    /// Returns a levitation velocity adjustment for `travelInAir`.
    fn levitation_travel_y_delta(&self, movement_y: f64) -> Option<f64> {
        self.mob_effect(vanilla_mob_effects::LEVITATION)
            .map(|effect| (0.05 * f64::from(effect.amplifier() + 1) - movement_y) * 0.2)
    }

    /// Returns whether vanilla `LivingEntity.travel()` should use fluid movement.
    fn should_travel_in_fluid(&self, fluid_state: FluidState) -> bool {
        (self.is_in_water() || self.is_in_lava())
            && self.is_affected_by_fluids()
            && !self.can_stand_on_fluid(fluid_state)
    }

    /// Returns vanilla `LivingEntity.getWaterSlowDown()`.
    fn get_water_slow_down(&self) -> f32 {
        0.8
    }

    /// Returns the water movement efficiency attribute used by fluid travel.
    fn water_movement_efficiency(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::WATER_MOVEMENT_EFFICIENCY)
            .unwrap_or(0.0) as f32
    }

    /// Returns whether dolphin's grace should apply to water travel.
    fn has_dolphins_grace(&self) -> bool {
        self.has_mob_effect(vanilla_mob_effects::DOLPHINS_GRACE)
    }

    /// Returns vanilla `LivingEntity.getFlyingSpeed()`.
    fn get_flying_speed(&self) -> f32 {
        if self
            .controlling_passenger()
            .is_some_and(|passenger| passenger.entity_type() == &vanilla_entities::PLAYER)
        {
            self.get_speed() * 0.1
        } else {
            0.02
        }
    }

    /// Returns vanilla `LivingEntity.getFrictionInfluencedSpeed()`.
    fn get_friction_influenced_speed(&self, block_friction: f32) -> f32 {
        if self.on_ground() {
            self.get_speed() * (0.216_000_02 / (block_friction * block_friction * block_friction))
        } else {
            self.get_flying_speed()
        }
    }

    /// Returns the vertical friction used by `travelInAir`.
    fn air_travel_vertical_friction(&self, _horizontal_friction: f32) -> f32 {
        // TODO: FlyingAnimal uses horizontal friction here once animal types exist.
        0.98
    }

    /// Applies vanilla `LivingEntity.handleOnClimbable()`.
    fn handle_on_climbable(&self, movement: DVec3) -> DVec3 {
        if !self.on_climbable() {
            return movement;
        }

        self.reset_fall_distance();
        let Some(world) = self.level() else {
            return movement;
        };
        let block_state = self.in_block_state(&world);
        let mut y = movement.y.max(-0.15);
        if y < 0.0
            && block_state.get_block() != &vanilla_blocks::SCAFFOLDING
            && self.is_suppressing_sliding_down_ladder()
            && self.entity_type() == &vanilla_entities::PLAYER
        {
            y = 0.0;
        }

        DVec3::new(
            movement.x.clamp(-0.15, 0.15),
            y,
            movement.z.clamp(-0.15, 0.15),
        )
    }

    /// Applies gravity using vanilla living-entity effective gravity.
    fn apply_living_travel_gravity(&self) {
        let gravity = self.get_effective_gravity();
        if gravity != 0.0 {
            let mut velocity = self.velocity();
            velocity.y -= gravity;
            self.set_velocity(velocity);
        }
    }

    /// Mirrors vanilla `LivingEntity.handleRelativeFrictionAndCalculateMovement()`.
    fn handle_relative_friction_and_calculate_movement(
        &self,
        input: DVec3,
        block_friction: f32,
    ) -> Option<(DVec3, MoveResult)> {
        self.move_relative(self.get_friction_influenced_speed(block_friction), input);
        self.set_velocity(self.handle_on_climbable(self.velocity()));
        let result = self.move_entity(MoverType::SelfMovement, self.velocity())?;
        let mut movement = self.velocity();
        if (result.horizontal_collision || self.is_jumping())
            && (self.on_climbable() || self.was_in_powder_snow() && self.can_walk_on_powder_snow())
        {
            movement.y = 0.2;
        }

        Some((movement, result))
    }

    /// Mirrors vanilla `LivingEntity.travelInAir()`.
    fn travel_in_air(&self, input: DVec3) -> Option<MoveResult> {
        let world = self.level()?;
        let pos_below = self.block_pos_below_that_affects_movement()?;
        let block_friction = if self.on_ground() {
            world.get_block_state(pos_below).get_block().config.friction
        } else {
            1.0
        };
        let horizontal_friction = block_friction * 0.91;
        let (movement, result) =
            self.handle_relative_friction_and_calculate_movement(input, block_friction)?;
        let movement_y = if let Some(levitation_y) = self.levitation_travel_y_delta(movement.y) {
            movement.y + levitation_y
        } else {
            movement.y - self.get_effective_gravity()
        };

        if self.should_discard_friction() {
            self.set_velocity(DVec3::new(movement.x, movement_y, movement.z));
        } else {
            let vertical_friction = self.air_travel_vertical_friction(horizontal_friction);
            self.set_velocity(DVec3::new(
                movement.x * f64::from(horizontal_friction),
                movement_y * f64::from(vertical_friction),
                movement.z * f64::from(horizontal_friction),
            ));
        }

        Some(result)
    }

    /// Mirrors vanilla `LivingEntity.getFluidFallingAdjustedMovement()`.
    fn get_fluid_falling_adjusted_movement(
        &self,
        base_gravity: f64,
        is_falling: bool,
        movement: DVec3,
    ) -> DVec3 {
        if base_gravity == 0.0 || self.is_sprinting() {
            return movement;
        }

        let y = if is_falling
            && (movement.y - 0.005).abs() >= 0.003
            && (movement.y - base_gravity / 16.0).abs() < 0.003
        {
            -0.003
        } else {
            movement.y - base_gravity / 16.0
        };

        DVec3::new(movement.x, y, movement.z)
    }

    /// Mirrors vanilla `LivingEntity.jumpOutOfFluid()`.
    fn jump_out_of_fluid(&self, old_y: f64) {
        if !self.horizontal_collision() {
            return;
        }

        let movement = self.velocity();
        let target_delta = DVec3::new(
            movement.x,
            movement.y + f64::from(0.6_f32) - self.position().y + old_y,
            movement.z,
        );
        if self.is_free(target_delta) {
            self.set_velocity(DVec3::new(movement.x, f64::from(0.3_f32), movement.z));
        }
    }

    /// Mirrors vanilla `LivingEntity.floatInWaterWhileRidden()`.
    fn float_in_water_while_ridden(&self) {
        if !REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::CAN_FLOAT_WHILE_RIDDEN)
        {
            return;
        }
        if !self.is_vehicle()
            || self.fluid_contact().water_height() <= self.get_fluid_jump_threshold()
        {
            return;
        }

        self.set_velocity(self.velocity() + DVec3::new(0.0, f64::from(0.04_f32), 0.0));
    }

    /// Mirrors vanilla `LivingEntity.travelInWater()`.
    fn travel_in_water(
        &self,
        input: DVec3,
        base_gravity: f64,
        is_falling: bool,
        old_y: f64,
    ) -> Option<MoveResult> {
        let mut slow_down = if self.is_sprinting() {
            0.9
        } else {
            self.get_water_slow_down()
        };
        let mut speed = 0.02;
        let mut water_movement_efficiency = self.water_movement_efficiency();
        if !self.on_ground() {
            water_movement_efficiency *= 0.5;
        }

        if water_movement_efficiency > 0.0 {
            slow_down += (0.546_000_06 - slow_down) * water_movement_efficiency;
            speed += (self.get_speed() - speed) * water_movement_efficiency;
        }

        if self.has_dolphins_grace() {
            slow_down = 0.96;
        }

        self.move_relative(speed, input);
        let result = self.move_entity(MoverType::SelfMovement, self.velocity())?;
        let mut movement = self.velocity();
        if result.horizontal_collision && self.on_climbable() {
            movement.y = 0.2;
        }

        movement = DVec3::new(
            movement.x * f64::from(slow_down),
            movement.y * f64::from(0.8_f32),
            movement.z * f64::from(slow_down),
        );
        self.set_velocity(self.get_fluid_falling_adjusted_movement(
            base_gravity,
            is_falling,
            movement,
        ));
        self.jump_out_of_fluid(old_y);

        Some(result)
    }

    /// Mirrors vanilla `LivingEntity.travelInLava()`.
    fn travel_in_lava(
        &self,
        input: DVec3,
        base_gravity: f64,
        is_falling: bool,
        old_y: f64,
    ) -> Option<MoveResult> {
        self.move_relative(0.02, input);
        let result = self.move_entity(MoverType::SelfMovement, self.velocity())?;
        if self.fluid_contact().lava_height() <= self.get_fluid_jump_threshold() {
            let movement = self.velocity();
            self.set_velocity(DVec3::new(
                movement.x * 0.5,
                movement.y * f64::from(0.8_f32),
                movement.z * 0.5,
            ));
            self.set_velocity(self.get_fluid_falling_adjusted_movement(
                base_gravity,
                is_falling,
                self.velocity(),
            ));
        } else {
            self.set_velocity(self.velocity() * 0.5);
        }

        if base_gravity != 0.0 {
            self.set_velocity(self.velocity() + DVec3::new(0.0, -base_gravity / 4.0, 0.0));
        }

        self.jump_out_of_fluid(old_y);

        Some(result)
    }

    /// Mirrors vanilla `LivingEntity.travelInFluid()`.
    fn travel_in_fluid(&self, input: DVec3) -> Option<MoveResult> {
        let is_falling = self.velocity().y <= 0.0;
        let old_y = self.position().y;
        let base_gravity = self.get_effective_gravity();
        if self.is_in_water() {
            let result = self.travel_in_water(input, base_gravity, is_falling, old_y);
            self.float_in_water_while_ridden();
            return result;
        }

        self.travel_in_lava(input, base_gravity, is_falling, old_y)
    }

    /// Mirrors the validation part of vanilla `LivingEntity.updateFallFlying()`.
    fn update_fall_flying(&self) {
        self.check_fall_distance_accumulation();
        if self.can_glide() {
            if let Some(free_fall_interval) =
                fall_flying_free_fall_interval(self.fall_flying_ticks())
            {
                if free_fall_interval % 2 == 0 {
                    self.damage_random_glider();
                }
                if let Some(world) = self.level() {
                    world.game_event_at(
                        &vanilla_game_events::ELYTRA_GLIDE,
                        self.position(),
                        &GameEventContext::new(Some(self.as_entity_event_source()), None),
                    );
                }
            }
        } else {
            self.set_fall_flying(false);
        }
    }

    /// Mirrors vanilla `LivingEntity.updateFallFlyingMovement()`.
    fn update_fall_flying_movement(&self, mut movement: DVec3) -> DVec3 {
        let look_angle = self.look_angle();
        let pitch_radians = self.rotation().1.to_radians();
        let look_horizontal_length = horizontal_distance(look_angle);
        let move_horizontal_length = horizontal_distance(movement);
        let gravity = self.get_effective_gravity();
        let lift_force = f64::from(pitch_radians).cos().powi(2);
        movement.y += gravity * (-1.0 + lift_force * 0.75);

        if movement.y < 0.0 && look_horizontal_length > 0.0 {
            let convert = movement.y * -0.1 * lift_force;
            movement += DVec3::new(
                look_angle.x * convert / look_horizontal_length,
                convert,
                look_angle.z * convert / look_horizontal_length,
            );
        }

        if pitch_radians < 0.0 && look_horizontal_length > 0.0 {
            let convert = move_horizontal_length * -f64::from(pitch_radians.sin()) * 0.04;
            movement += DVec3::new(
                -look_angle.x * convert / look_horizontal_length,
                convert * 3.2,
                -look_angle.z * convert / look_horizontal_length,
            );
        }

        if look_horizontal_length > 0.0 {
            movement += DVec3::new(
                (look_angle.x / look_horizontal_length * move_horizontal_length - movement.x) * 0.1,
                0.0,
                (look_angle.z / look_horizontal_length * move_horizontal_length - movement.z) * 0.1,
            );
        }

        DVec3::new(
            movement.x * f64::from(0.99_f32),
            movement.y * f64::from(0.98_f32),
            movement.z * f64::from(0.99_f32),
        )
    }

    /// Mirrors vanilla `LivingEntity.stopFallFlying()`.
    fn stop_fall_flying(&self) {
        self.set_fall_flying(true);
        self.set_fall_flying(false);
    }

    /// Mirrors vanilla `LivingEntity.handleFallFlyingCollisions()`.
    fn handle_fall_flying_collisions(
        &self,
        previous_horizontal_speed: f64,
        new_horizontal_speed: f64,
    ) {
        if !self.horizontal_collision() {
            return;
        }

        let damage = fall_flying_collision_damage(previous_horizontal_speed, new_horizontal_speed);
        if damage <= 0.0 {
            return;
        }

        self.play_sound(self.fall_damage_sound(damage as i32), 1.0, 1.0);
        self.hurt(
            &DamageSource::environment(&vanilla_damage_types::FLY_INTO_WALL),
            damage,
        );
    }

    /// Mirrors vanilla `LivingEntity.travelFallFlying()`.
    fn travel_fall_flying(&self, input: DVec3) -> Option<MoveResult> {
        if self.on_climbable() {
            let result = self.travel_in_air(input);
            self.stop_fall_flying();
            return result;
        }

        let previous_movement = self.velocity();
        let previous_horizontal_speed = horizontal_distance(previous_movement);
        self.set_velocity(self.update_fall_flying_movement(previous_movement));
        let result = self.move_entity(MoverType::SelfMovement, self.velocity());
        let new_horizontal_speed = horizontal_distance(self.velocity());
        self.handle_fall_flying_collisions(previous_horizontal_speed, new_horizontal_speed);
        result
    }

    /// Default vanilla `LivingEntity.travel()` implementation for overrides.
    fn default_travel(&self, input: DVec3) -> Option<MoveResult> {
        let world = self.level()?;
        let fluid_state = get_fluid_state(&world, self.block_position());
        if self.should_travel_in_fluid(fluid_state) {
            return self.travel_in_fluid(input);
        }
        if self.is_fall_flying() {
            return self.travel_fall_flying(input);
        }

        self.travel_in_air(input)
    }

    /// Mirrors vanilla `LivingEntity.travel()`.
    fn travel(&self, input: DVec3) -> Option<MoveResult> {
        self.default_travel(input)
    }

    /// Returns the bed position that makes this living entity sleeping.
    fn sleeping_pos(&self) -> Option<BlockPos> {
        self.living_base().sleeping_pos()
    }

    /// Sets the vanilla living-entity sleeping position.
    fn set_sleeping_pos(&self, bed_position: BlockPos) {
        self.living_base().set_sleeping_pos(bed_position);
    }

    /// Clears the vanilla living-entity sleeping position.
    fn clear_sleeping_pos(&self) {
        self.living_base().clear_sleeping_pos();
    }

    /// Checks if the entity is sleeping.
    fn is_sleeping(&self) -> bool {
        self.sleeping_pos().is_some()
    }

    /// Stops the entity from sleeping.
    fn stop_sleeping(&self) {
        self.clear_sleeping_pos();
    }

    /// Checks if the entity is sprinting.
    fn is_sprinting(&self) -> bool {
        self.living_base().is_sprinting()
    }

    /// Sets whether the entity is sprinting.
    fn set_sprinting(&self, sprinting: bool) {
        self.set_shared_sprinting(sprinting);
        self.living_base().set_sprinting(sprinting);
    }

    /// Gets the entity's cached movement speed.
    fn get_speed(&self) -> f32 {
        self.living_base().speed()
    }

    /// Sets the entity's cached movement speed.
    fn set_speed(&self, speed: f32) {
        self.living_base().set_speed(speed);
    }

    /// Applies vanilla post-impulse movement validation grace.
    fn apply_post_impulse_grace_time(&self, ticks: i32) {
        self.living_base().apply_post_impulse_grace_time(ticks);
    }

    /// Returns whether movement validation is inside post-impulse grace.
    fn is_in_post_impulse_grace_time(&self) -> bool {
        self.living_base().is_in_post_impulse_grace_time()
    }

    /// Decrements post-impulse grace once per living-entity tick.
    fn tick_post_impulse_grace_time(&self) {
        self.living_base().tick_post_impulse_grace_time();
    }

    /// Drains dirty attributes and applies server-side effects.
    fn refresh_dirty_attributes(&self) {
        let dirty = self.attributes().lock().drain_dirty_updates();
        for attr in dirty {
            if attr.key == vanilla_attributes::MAX_HEALTH.key {
                let max = self.get_max_health();
                if self.get_health() > max {
                    self.set_health(max);
                }
            } else if attr.key == vanilla_attributes::MAX_ABSORPTION.key {
                let max = self
                    .attributes()
                    .lock()
                    .get_value(vanilla_attributes::MAX_ABSORPTION)
                    .unwrap_or(0.0) as f32;
                if self.get_absorption_amount() > max {
                    self.set_absorption_amount(max);
                }
            }
            // TODO: SCALE → refreshDimensions()
            // TODO: WAYPOINT_TRANSMIT_RANGE → waypoint manager
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::blocks::{
        block_state_ext::BlockStateExt as _,
        properties::{BlockStateProperties, Direction as BlockDirection},
    };
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::fluid::FluidState;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{
        sound_events, test_support::init_test_registry, vanilla_attributes, vanilla_blocks,
        vanilla_entities, vanilla_fluids, vanilla_items, vanilla_mob_effects,
    };
    use steel_utils::{BlockPos, Direction};

    use crate::inventory::equipment::EquipmentSlot;

    use super::{
        Entity, EntityBase, EntityFluidContact, EntityLevelCallback, EntityMoveError,
        EntityVerticalMovementStateUpdate, LivingEntity, LivingEntityBase, LivingTravelInput,
        RemovalReason, SharedEntity, closest_open_space_direction, fall_damage_reset_clip_target,
        fall_flying_collision_damage, fall_flying_free_fall_interval, get_input_vector,
        should_apply_resolved_movement, trapdoor_usable_as_ladder_state,
    };

    struct PushableTestEntity {
        base: EntityBase,
    }

    impl PushableTestEntity {
        fn shared(id: i32, position: DVec3) -> SharedEntity {
            Arc::new(Self {
                base: EntityBase::new(id, position, vanilla_entities::ITEM.dimensions, Weak::new()),
            })
        }
    }

    impl Entity for PushableTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ITEM
        }

        fn is_pushable(&self) -> bool {
            true
        }
    }

    struct CommitRejectingCallback {
        entity_id: i32,
    }

    impl EntityLevelCallback for CommitRejectingCallback {
        fn validate_move(&self, _old_pos: DVec3, _new_pos: DVec3) -> Result<(), EntityMoveError> {
            Ok(())
        }

        fn on_move_committed(
            &self,
            _old_pos: DVec3,
            _new_pos: DVec3,
        ) -> Result<(), EntityMoveError> {
            Err(EntityMoveError::NotLive {
                entity_id: self.entity_id,
            })
        }

        fn on_remove(&self, _reason: RemovalReason) {}
    }

    struct KnownMovementTestEntity {
        base: EntityBase,
        entity_type: EntityTypeRef,
        known_movement: DVec3,
        known_speed: DVec3,
        uses_client_movement_packets: bool,
    }

    impl KnownMovementTestEntity {
        fn shared(
            id: i32,
            entity_type: EntityTypeRef,
            known_movement: DVec3,
            known_speed: DVec3,
        ) -> SharedEntity {
            Arc::new(Self {
                base: EntityBase::new(id, DVec3::ZERO, entity_type.dimensions, Weak::new()),
                entity_type,
                known_movement,
                known_speed,
                uses_client_movement_packets: entity_type == &vanilla_entities::PLAYER,
            })
        }
    }

    impl Entity for KnownMovementTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }

        fn known_movement(&self) -> DVec3 {
            self.known_movement
        }

        fn known_speed(&self) -> DVec3 {
            self.known_speed
        }

        fn uses_client_movement_packets(&self) -> bool {
            self.uses_client_movement_packets
        }
    }

    struct LivingFluidTestEntity {
        base: EntityBase,
        living_base: LivingEntityBase,
        entity_type: EntityTypeRef,
        affected_by_fluids: bool,
        can_stand_on_fluid: bool,
        vehicle: bool,
    }

    impl LivingFluidTestEntity {
        fn new(water_height: f64, lava_height: f64, affected_by_fluids: bool) -> Self {
            let base = EntityBase::new(
                1,
                DVec3::ZERO,
                vanilla_entities::PLAYER.dimensions,
                Weak::new(),
            );
            base.set_fluid_contact(EntityFluidContact::from_parts(
                water_height,
                lava_height,
                false,
                false,
            ));
            Self {
                base,
                living_base: LivingEntityBase::new(&vanilla_entities::PLAYER),
                entity_type: &vanilla_entities::PLAYER,
                affected_by_fluids,
                can_stand_on_fluid: false,
                vehicle: false,
            }
        }

        const fn with_standing_on_fluid(mut self) -> Self {
            self.can_stand_on_fluid = true;
            self
        }

        const fn with_entity_type(mut self, entity_type: EntityTypeRef) -> Self {
            self.entity_type = entity_type;
            self
        }

        const fn with_vehicle(mut self) -> Self {
            self.vehicle = true;
            self
        }

        fn equip(&self, slot: EquipmentSlot, stack: ItemStack) {
            self.living_base.equipment().lock().set(slot, stack);
        }
    }

    impl Entity for LivingFluidTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }

        fn is_living_entity(&self) -> bool {
            true
        }

        fn is_vehicle(&self) -> bool {
            self.vehicle
        }

        fn get_default_gravity(&self) -> f64 {
            LivingEntity::get_attribute_gravity(self)
        }
    }

    impl LivingEntity for LivingFluidTestEntity {
        fn living_base(&self) -> &LivingEntityBase {
            &self.living_base
        }

        fn get_health(&self) -> f32 {
            20.0
        }

        fn set_health(&self, _health: f32) {}

        fn get_absorption_amount(&self) -> f32 {
            0.0
        }

        fn set_absorption_amount(&self, _amount: f32) {}

        fn is_affected_by_fluids(&self) -> bool {
            self.affected_by_fluids
        }

        fn can_stand_on_fluid(&self, _fluid_state: FluidState) -> bool {
            self.can_stand_on_fluid
        }
    }

    struct ControlledVehicleTestEntity {
        base: EntityBase,
        controller: Option<SharedEntity>,
    }

    impl ControlledVehicleTestEntity {
        fn shared(id: i32, controller: Option<SharedEntity>) -> SharedEntity {
            Arc::new(Self {
                base: EntityBase::new(
                    id,
                    DVec3::ZERO,
                    vanilla_entities::ACACIA_BOAT.dimensions,
                    Weak::new(),
                ),
                controller,
            })
        }
    }

    impl Entity for ControlledVehicleTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ACACIA_BOAT
        }

        fn controlling_passenger(&self) -> Option<SharedEntity> {
            self.controller.clone()
        }
    }

    fn assert_vec3_close(left: DVec3, right: DVec3) {
        let diff = left - right;
        assert!(
            diff.length_squared() < 1.0e-12,
            "expected {left:?} to equal {right:?}"
        );
    }

    fn closest_direction_with_blocked_neighbors(
        fractional_position: DVec3,
        blocked_directions: &[Direction],
    ) -> Direction {
        let origin = BlockPos::ZERO;
        closest_open_space_direction(origin, fractional_position, |neighbor_pos| {
            blocked_directions
                .iter()
                .any(|direction| direction.relative(origin) == neighbor_pos)
        })
    }

    #[test]
    fn default_tick_runs_vanilla_entity_base_tick() {
        let entity = PushableTestEntity::shared(1, DVec3::ZERO);
        entity.base().set_boarding_cooldown(2);

        entity.default_tick();

        assert_eq!(entity.base().boarding_cooldown(), 1);
    }

    #[test]
    fn closest_open_space_direction_matches_vanilla_order_on_ties() {
        assert_eq!(
            closest_direction_with_blocked_neighbors(DVec3::splat(0.5), &[]),
            Direction::North
        );
    }

    #[test]
    fn closest_open_space_direction_skips_full_collision_neighbors() {
        assert_eq!(
            closest_direction_with_blocked_neighbors(
                DVec3::new(0.3, 0.5, 0.7),
                &[Direction::South]
            ),
            Direction::West
        );
        assert_eq!(
            closest_direction_with_blocked_neighbors(
                DVec3::new(0.3, 0.2, 0.7),
                &[
                    Direction::North,
                    Direction::South,
                    Direction::West,
                    Direction::East,
                ],
            ),
            Direction::Up
        );
    }

    #[test]
    fn resolved_movement_application_matches_vanilla_threshold() {
        assert!(should_apply_resolved_movement(DVec3::ZERO, DVec3::ZERO));
        assert!(should_apply_resolved_movement(
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(1.0e-3, 0.0, 0.0)
        ));
        assert!(!should_apply_resolved_movement(
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::ZERO
        ));
    }

    #[test]
    fn move_without_physics_returns_none_when_position_commit_rejects() {
        init_test_registry();
        let entity = PushableTestEntity::shared(1, DVec3::ZERO);
        entity.set_no_physics(true);
        entity.set_level_callback(Arc::new(CommitRejectingCallback {
            entity_id: entity.id(),
        }));

        let result = entity.move_without_physics(DVec3::new(1.0, 0.0, 0.0));

        assert!(result.is_none());
        assert_vec3_close(entity.position(), DVec3::ZERO);
    }

    #[test]
    fn fall_damage_reset_clip_target_matches_vanilla_thresholds() {
        let position = DVec3::new(1.0, 2.0, 3.0);

        assert_eq!(
            fall_damage_reset_clip_target(position, DVec3::new(1.0, 0.0, 0.0), 0.0),
            None
        );
        assert_eq!(
            fall_damage_reset_clip_target(position, DVec3::new(0.999, 0.0, 0.0), 2.0),
            None
        );
        assert_eq!(
            fall_damage_reset_clip_target(position, DVec3::new(1.0, 0.0, 0.0), 2.0),
            Some(DVec3::new(2.0, 2.0, 3.0))
        );
        assert_eq!(
            fall_damage_reset_clip_target(position, DVec3::new(10.0, 0.0, 0.0), 2.0),
            Some(DVec3::new(9.0, 2.0, 3.0))
        );
    }

    #[test]
    fn input_vector_ignores_tiny_input_like_vanilla() {
        assert_vec3_close(
            get_input_vector(DVec3::new(1.0E-4, 0.0, 0.0), 0.02, 0.0),
            DVec3::ZERO,
        );
    }

    #[test]
    fn input_vector_normalizes_large_input_and_rotates_by_yaw() {
        assert_vec3_close(
            get_input_vector(DVec3::new(2.0, 0.0, 0.0), 0.5, 0.0),
            DVec3::new(0.5, 0.0, 0.0),
        );
        assert_vec3_close(
            get_input_vector(DVec3::new(0.0, 0.0, 1.0), 0.5, 90.0),
            DVec3::new(-0.5, 0.0, 0.0),
        );
    }

    #[test]
    fn look_angle_matches_vanilla_view_vector_axes() {
        let entity = PushableTestEntity::shared(1, DVec3::ZERO);

        entity.set_rotation((0.0, 0.0));
        assert_vec3_close(entity.look_angle(), DVec3::new(0.0, 0.0, 1.0));

        entity.set_rotation((90.0, 0.0));
        assert_vec3_close(entity.look_angle(), DVec3::new(-1.0, 0.0, 0.0));

        entity.set_rotation((0.0, 90.0));
        assert_vec3_close(entity.look_angle(), DVec3::new(0.0, -1.0, 0.0));
    }

    #[test]
    fn fall_flying_movement_applies_vanilla_gravity_lift_and_drag() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_rotation((0.0, 0.0));

        assert_vec3_close(
            entity.update_fall_flying_movement(DVec3::ZERO),
            DVec3::new(
                0.0,
                -0.018 * f64::from(0.98_f32),
                0.0018 * f64::from(0.99_f32),
            ),
        );
    }

    #[test]
    fn fall_flying_movement_converts_upward_pitch_to_lift() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_rotation((0.0, -45.0));

        let movement = entity.update_fall_flying_movement(DVec3::new(0.0, -0.2, 0.4));

        assert!(movement.y > -0.2);
        assert!(movement.z > 0.0);
    }

    #[test]
    fn fall_flying_collision_damage_matches_vanilla_threshold() {
        assert!(fall_flying_collision_damage(1.0, 0.8) <= 0.0);
        assert!((fall_flying_collision_damage(1.0, 0.6) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn fall_flying_free_fall_interval_matches_vanilla_cadence() {
        assert_eq!(fall_flying_free_fall_interval(8), None);
        assert_eq!(fall_flying_free_fall_interval(9), Some(1));
        assert_eq!(fall_flying_free_fall_interval(19), Some(2));
    }

    #[test]
    fn jump_boost_power_uses_active_effect_amplifier() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

        assert!(entity.get_jump_boost_power().abs() < f32::EPSILON);

        entity.set_mob_effect(vanilla_mob_effects::JUMP_BOOST, 2);

        assert!((entity.get_jump_boost_power() - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn levitation_travel_uses_active_effect_amplifier() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

        assert!(entity.levitation_travel_y_delta(-0.2).is_none());

        entity.set_mob_effect(vanilla_mob_effects::LEVITATION, 1);

        assert!(
            (entity.levitation_travel_y_delta(-0.2).unwrap_or(0.0) - 0.06).abs() < f64::EPSILON
        );
    }

    #[test]
    fn slow_falling_caps_effective_gravity_only_while_falling() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_mob_effect_active(vanilla_mob_effects::SLOW_FALLING, true);
        entity.set_velocity(DVec3::new(0.0, -0.1, 0.0));

        assert!((entity.get_effective_gravity() - 0.01).abs() < f64::EPSILON);

        entity.set_velocity(DVec3::new(0.0, 0.1, 0.0));

        assert!((entity.get_effective_gravity() - entity.get_gravity()).abs() < f64::EPSILON);
    }

    #[test]
    fn fall_distance_accumulation_clamps_like_vanilla() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_fall_distance(2.0);
        entity.set_velocity(DVec3::new(0.0, -0.4, 0.0));

        entity.check_fall_distance_accumulation();

        assert!((entity.fall_distance() - 1.0).abs() < f64::EPSILON);

        entity.set_fall_distance(2.0);
        entity.set_velocity(DVec3::new(0.0, -0.6, 0.0));

        entity.check_fall_distance_accumulation();

        assert!((entity.fall_distance() - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn can_glide_using_matches_vanilla_component_gate() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        let mut elytra = ItemStack::new(&vanilla_items::ITEMS.elytra);

        assert!(entity.can_glide_using(&elytra, EquipmentSlot::Chest));
        assert!(!entity.can_glide_using(&elytra, EquipmentSlot::Head));

        elytra.set_damage_value(elytra.get_max_damage() - 1);

        assert!(elytra.next_damage_will_break());
        assert!(!entity.can_glide_using(&elytra, EquipmentSlot::Chest));
        assert!(!entity.can_glide_using(
            &ItemStack::new(&vanilla_items::ITEMS.stone),
            EquipmentSlot::Chest
        ));
    }

    #[test]
    fn living_freeze_immunity_uses_armor_equipment() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

        assert!(entity.default_living_can_freeze());

        entity.equip(
            EquipmentSlot::Feet,
            ItemStack::new(&vanilla_items::ITEMS.leather_boots),
        );

        assert!(!entity.default_living_can_freeze());
    }

    #[test]
    fn living_freeze_immunity_ignores_non_armor_equipment() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.equip(
            EquipmentSlot::MainHand,
            ItemStack::new(&vanilla_items::ITEMS.leather_boots),
        );

        assert!(entity.default_living_can_freeze());
    }

    #[test]
    fn living_powder_snow_walkability_uses_feet_equipment() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

        assert!(!entity.default_living_can_walk_on_powder_snow());

        entity.equip(
            EquipmentSlot::Feet,
            ItemStack::new(&vanilla_items::ITEMS.leather_boots),
        );

        assert!(entity.default_living_can_walk_on_powder_snow());
    }

    #[test]
    fn living_powder_snow_walkability_ignores_non_feet_equipment() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.equip(
            EquipmentSlot::MainHand,
            ItemStack::new(&vanilla_items::ITEMS.leather_boots),
        );

        assert!(!entity.default_living_can_walk_on_powder_snow());
    }

    #[test]
    fn default_can_glide_uses_living_equipment() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_on_ground(false);

        assert!(!entity.can_glide());

        entity.equip(
            EquipmentSlot::Chest,
            ItemStack::new(&vanilla_items::ITEMS.elytra),
        );

        assert!(entity.can_glide());
    }

    #[test]
    fn try_to_start_fall_flying_uses_vanilla_glider_gate() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.equip(
            EquipmentSlot::Chest,
            ItemStack::new(&vanilla_items::ITEMS.elytra),
        );
        entity.set_on_ground(false);

        assert!(entity.try_to_start_fall_flying());
        assert!(entity.is_fall_flying());
    }

    #[test]
    fn try_to_start_fall_flying_rejects_levitation() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.equip(
            EquipmentSlot::Chest,
            ItemStack::new(&vanilla_items::ITEMS.elytra),
        );
        entity.set_on_ground(false);
        entity.set_mob_effect_active(vanilla_mob_effects::LEVITATION, true);

        assert!(!entity.try_to_start_fall_flying());
        assert!(!entity.is_fall_flying());
    }

    #[test]
    fn update_fall_flying_damages_glider_every_second_event_interval() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.equip(
            EquipmentSlot::Chest,
            ItemStack::new(&vanilla_items::ITEMS.elytra),
        );
        entity.set_on_ground(false);
        for _ in 0..19 {
            entity.living_base.tick_fall_flying_state(true);
        }

        entity.update_fall_flying();

        assert_eq!(
            entity
                .living_base
                .equipment()
                .lock()
                .get_ref(EquipmentSlot::Chest)
                .get_damage_value(),
            1
        );
    }

    #[test]
    fn update_fall_flying_stops_when_glider_gate_fails() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_fall_flying(true);

        entity.update_fall_flying();

        assert!(!entity.is_fall_flying());
    }

    #[test]
    fn fall_damage_sound_selects_vanilla_small_and_big_sounds() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

        assert_eq!(
            entity.fall_damage_sound(4),
            &sound_events::ENTITY_GENERIC_SMALL_FALL
        );
        assert_eq!(
            entity.fall_damage_sound(5),
            &sound_events::ENTITY_GENERIC_BIG_FALL
        );
    }

    #[test]
    fn stop_fall_flying_toggles_shared_state_back_to_false() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_fall_flying(true);

        entity.stop_fall_flying();

        assert!(!entity.is_fall_flying());
    }

    #[test]
    fn fluid_falling_adjustment_matches_vanilla_special_falling_case() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

        let movement =
            entity.get_fluid_falling_adjusted_movement(0.16, true, DVec3::new(1.0, 0.01, 1.0));

        assert_vec3_close(movement, DVec3::new(1.0, -0.003, 1.0));
    }

    #[test]
    fn fluid_falling_adjustment_is_skipped_while_sprinting() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_sprinting(true);

        let movement =
            entity.get_fluid_falling_adjusted_movement(0.16, true, DVec3::new(1.0, 0.01, 1.0));

        assert_vec3_close(movement, DVec3::new(1.0, 0.01, 1.0));
    }

    #[test]
    fn water_float_while_ridden_uses_vanilla_entity_type_tag_and_threshold() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.5, 0.0, true)
            .with_entity_type(&vanilla_entities::HORSE)
            .with_vehicle();

        entity.float_in_water_while_ridden();

        assert_vec3_close(entity.velocity(), DVec3::new(0.0, f64::from(0.04_f32), 0.0));
    }

    #[test]
    fn water_float_while_ridden_ignores_non_vehicle_tagged_entity() {
        init_test_registry();
        let entity =
            LivingFluidTestEntity::new(0.5, 0.0, true).with_entity_type(&vanilla_entities::HORSE);

        entity.float_in_water_while_ridden();

        assert_vec3_close(entity.velocity(), DVec3::ZERO);
    }

    #[test]
    fn dolphins_grace_water_travel_hook_uses_active_mob_effect_state() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.5, 0.0, true);

        assert!(!entity.has_dolphins_grace());
        entity.set_mob_effect_active(vanilla_mob_effects::DOLPHINS_GRACE, true);
        assert!(entity.has_dolphins_grace());
    }

    #[test]
    fn jump_from_ground_uses_jump_strength_and_marks_velocity_sync() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        let jump_strength = f64::from(vanilla_attributes::JUMP_STRENGTH.default_value as f32);

        entity.jump_from_ground();

        assert_vec3_close(entity.velocity(), DVec3::new(0.0, jump_strength, 0.0));
        assert!(entity.needs_velocity_sync());
    }

    #[test]
    fn sprint_jump_from_ground_adds_vanilla_horizontal_impulse() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        let jump_strength = f64::from(vanilla_attributes::JUMP_STRENGTH.default_value as f32);
        entity.set_sprinting(true);
        entity.set_rotation((0.0, 0.0));

        entity.jump_from_ground();

        assert_vec3_close(
            entity.velocity(),
            DVec3::new(0.0, jump_strength, f64::from(0.2_f32)),
        );
    }

    #[test]
    fn living_jump_in_water_uses_fluid_jump_impulse_without_cooldown() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.5, 0.0, true);
        entity.set_jumping(true);

        entity.handle_living_jump();

        assert_vec3_close(entity.velocity(), DVec3::new(0.0, f64::from(0.04_f32), 0.0));
        assert_eq!(entity.no_jump_delay(), 0);
    }

    #[test]
    fn living_jump_without_input_resets_jump_delay_like_vanilla() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_no_jump_delay(4);

        entity.handle_living_jump();

        assert_eq!(entity.no_jump_delay(), 0);
    }

    #[test]
    fn living_ai_step_zeroes_tiny_player_velocity_like_vanilla() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_velocity(DVec3::new(0.002, 0.002, 0.002));

        entity.apply_living_velocity_thresholds();

        assert_vec3_close(entity.velocity(), DVec3::ZERO);
    }

    #[test]
    fn living_ai_step_keeps_player_horizontal_velocity_above_combined_threshold() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        let velocity = DVec3::new(0.002, 0.003, 0.0025);
        entity.set_velocity(velocity);

        entity.apply_living_velocity_thresholds();

        assert_vec3_close(entity.velocity(), velocity);
    }

    #[test]
    fn default_ai_step_resets_idle_jump_delay_and_dampens_input_before_travel() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_no_jump_delay(2);
        entity.set_travel_input(LivingTravelInput::new(1.0, 0.5, -1.0));

        assert!(entity.default_ai_step().is_none());

        assert_eq!(entity.no_jump_delay(), 0);
        assert_eq!(
            entity.travel_input(),
            LivingTravelInput::new(0.98, 0.5, -0.98)
        );
    }

    #[test]
    fn default_ai_step_jumps_from_ground_and_sets_vanilla_cooldown() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        let jump_strength = f64::from(vanilla_attributes::JUMP_STRENGTH.default_value as f32);
        entity.set_on_ground(true);
        entity.set_jumping(true);

        assert!(entity.default_ai_step().is_none());

        assert_vec3_close(entity.velocity(), DVec3::new(0.0, jump_strength, 0.0));
        assert_eq!(entity.no_jump_delay(), 10);
        assert!(entity.needs_velocity_sync());
    }

    #[test]
    fn living_travel_fluid_predicate_matches_vanilla_hooks() {
        init_test_registry();
        let water = FluidState::source(&vanilla_fluids::WATER);

        assert!(LivingFluidTestEntity::new(0.4, 0.0, true).should_travel_in_fluid(water));
        assert!(LivingFluidTestEntity::new(0.0, 0.4, true).should_travel_in_fluid(water));
        assert!(!LivingFluidTestEntity::new(0.0, 0.0, true).should_travel_in_fluid(water));
        assert!(!LivingFluidTestEntity::new(0.4, 0.0, false).should_travel_in_fluid(water));
        assert!(
            !LivingFluidTestEntity::new(0.4, 0.0, true)
                .with_standing_on_fluid()
                .should_travel_in_fluid(water)
        );
    }

    #[test]
    fn open_trapdoor_matches_ladder_facing_for_climbable() {
        init_test_registry();

        let trapdoor = vanilla_blocks::OAK_TRAPDOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, true)
            .set_value(&BlockStateProperties::FACING, BlockDirection::North);
        let ladder = vanilla_blocks::LADDER
            .default_state()
            .set_value(&BlockStateProperties::FACING, BlockDirection::North);

        assert!(trapdoor_usable_as_ladder_state(trapdoor, ladder));
    }

    #[test]
    fn closed_trapdoor_is_not_usable_as_ladder() {
        init_test_registry();

        let trapdoor = vanilla_blocks::OAK_TRAPDOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, false)
            .set_value(&BlockStateProperties::FACING, BlockDirection::North);
        let ladder = vanilla_blocks::LADDER
            .default_state()
            .set_value(&BlockStateProperties::FACING, BlockDirection::North);

        assert!(!trapdoor_usable_as_ladder_state(trapdoor, ladder));
    }

    #[test]
    fn trapdoor_ladder_facing_must_match() {
        init_test_registry();

        let trapdoor = vanilla_blocks::OAK_TRAPDOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, true)
            .set_value(&BlockStateProperties::FACING, BlockDirection::North);
        let ladder = vanilla_blocks::LADDER
            .default_state()
            .set_value(&BlockStateProperties::FACING, BlockDirection::South);

        assert!(!trapdoor_usable_as_ladder_state(trapdoor, ladder));
    }

    #[test]
    fn vertical_collision_state_update_matches_vanilla_authority_gate() {
        assert!(
            EntityVerticalMovementStateUpdate::for_move(DVec3::new(0.0, -0.1, 0.0), false)
                .refreshes_state()
        );
        assert!(EntityVerticalMovementStateUpdate::for_move(DVec3::ZERO, true).refreshes_state());
        assert!(
            !EntityVerticalMovementStateUpdate::for_move(DVec3::new(0.1, 0.0, 0.0), false)
                .refreshes_state()
        );
    }

    #[test]
    fn push_impulse_updates_velocity_and_marks_sync() {
        let entity = PushableTestEntity::shared(1, DVec3::ZERO);

        entity.push_impulse(DVec3::new(0.1, 0.2, 0.3));

        assert_vec3_close(entity.velocity(), DVec3::new(0.1, 0.2, 0.3));
        assert!(entity.needs_velocity_sync());

        entity.clear_velocity_sync();
        entity.push_impulse(DVec3::new(f64::INFINITY, 0.0, 0.0));

        assert_vec3_close(entity.velocity(), DVec3::new(0.1, 0.2, 0.3));
        assert!(!entity.needs_velocity_sync());
    }

    #[test]
    fn default_below_world_hook_discards_entity() {
        let entity = PushableTestEntity::shared(1, DVec3::ZERO);

        entity.on_below_world();

        assert!(entity.is_removed());
    }

    #[test]
    fn base_entity_has_no_controlling_passenger() {
        let entity = PushableTestEntity::shared(1, DVec3::ZERO);

        assert!(entity.controlling_passenger().is_none());
        assert!(!entity.has_controlling_passenger());
    }

    #[test]
    fn controlled_vehicle_uses_player_known_movement_and_speed() {
        let player_movement = DVec3::new(0.25, 0.0, -0.5);
        let player_speed = DVec3::new(0.5, 0.0, -1.0);
        let controller = KnownMovementTestEntity::shared(
            1,
            &vanilla_entities::PLAYER,
            player_movement,
            player_speed,
        );
        let vehicle = ControlledVehicleTestEntity::shared(2, Some(controller));

        assert!(vehicle.uses_client_movement_packets());
        assert!(!vehicle.is_server_driven_movement());
        assert!(!vehicle.can_simulate_movement());
        assert!(!vehicle.is_effective_ai());

        vehicle.set_velocity(DVec3::new(4.0, 0.0, 4.0));
        vehicle.base().advance_base_tick_state();
        vehicle.base().set_position_local(DVec3::new(2.0, 0.0, 0.0));
        vehicle.base().advance_base_tick_state();

        assert!(vehicle.has_controlling_passenger());
        assert_vec3_close(vehicle.known_movement(), player_movement);
        assert_vec3_close(vehicle.known_speed(), player_speed);

        vehicle.set_removed(RemovalReason::Discarded);

        assert_vec3_close(vehicle.known_movement(), DVec3::new(4.0, 0.0, 4.0));
        assert_vec3_close(vehicle.known_speed(), DVec3::new(2.0, 0.0, 0.0));
    }

    #[test]
    fn controlled_vehicle_known_movement_falls_back_without_active_player_controller() {
        let non_player_controller = KnownMovementTestEntity::shared(
            1,
            &vanilla_entities::ZOMBIE,
            DVec3::new(0.25, 0.0, -0.5),
            DVec3::new(0.5, 0.0, -1.0),
        );
        let vehicle = ControlledVehicleTestEntity::shared(2, Some(non_player_controller));
        vehicle.set_velocity(DVec3::new(4.0, 0.0, 4.0));
        vehicle.base().advance_base_tick_state();
        vehicle.base().set_position_local(DVec3::new(2.0, 0.0, 0.0));
        vehicle.base().advance_base_tick_state();

        assert_vec3_close(vehicle.known_movement(), DVec3::new(4.0, 0.0, 4.0));
        assert_vec3_close(vehicle.known_speed(), DVec3::new(2.0, 0.0, 0.0));
    }

    #[test]
    fn push_entity_separates_pushable_entities_like_vanilla() {
        let left = PushableTestEntity::shared(1, DVec3::ZERO);
        let right = PushableTestEntity::shared(2, DVec3::new(1.0, 0.0, 0.0));

        left.push_entity(right.as_ref());

        assert_vec3_close(left.velocity(), DVec3::new(-0.05, 0.0, 0.0));
        assert_vec3_close(right.velocity(), DVec3::new(0.05, 0.0, 0.0));
        assert!(left.needs_velocity_sync());
        assert!(right.needs_velocity_sync());
    }
}
