//! This module contains entity-related traits and types.

use std::{
    borrow::Cow,
    sync::{Arc, LazyLock, Weak},
};

use glam::DVec3;
use rand::{SeedableRng as _, rngs::StdRng};
use rustc_hash::FxHashSet;
use simdnbt::ToNbtTag as _;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_protocol::packets::game::{
    AnimateAction, AttributeSnapshot, CAnimate, CDamageEvent, CEntityEvent, CHurtAnimation,
    CTeleportEntity, EquipmentSlotItem, RelativeMovement, SoundSource,
};
use steel_registry::blocks::{
    block_state_ext::BlockStateExt as _, properties::BlockStateProperties,
    shapes::is_shape_full_block,
};
use steel_registry::data_components::vanilla_components::GLIDER;
use steel_registry::enchantment_effect::EnchantmentEffectComponent;
use steel_registry::entity_data::{DataValue, EntityPose};
use steel_registry::entity_type::{EntityAttachment, EntityDimensions, EntityTypeRef};
use steel_registry::fluid::{FluidState, FluidStateExt as _};
use steel_registry::game_events::GameEventRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::loot_table::{
    DamageSourceInfo, EntityRef, EntityRefFlags, LootContext, LootTableRef,
};
use steel_registry::mob_effect::MobEffectRef;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_type_tags::EntityTypeTag;
use steel_registry::vanilla_game_rules::{MAX_ENTITY_CRAMMING, MOB_DROPS};
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, sound_events, vanilla_damage_type_tags, vanilla_damage_types,
    vanilla_game_events,
};
use steel_registry::{RegistryEntry, RegistryExt};
use steel_registry::{vanilla_attributes, vanilla_fluid_tags, vanilla_items, vanilla_mob_effects};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::types::{Difficulty, InteractionHand};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, Direction, ErasedType, Identifier, UuidExt as _, WorldAabb,
    axis::Axis, block_util::FoundRectangle, text::DisplayResolutor,
};
use text_components::{
    Modifier as _, TextComponent, interactivity::HoverEvent, translation::TranslatedMessage,
};
use uuid::Uuid;

use crate::behavior::{
    BLOCK_BEHAVIORS, BlockCollisionContext, BlockStateBehaviorExt as _, EntityFallOnContext,
    EntityLandingContext, FLUID_BEHAVIORS, InteractionResult,
};
use crate::chunk_saver::ChunkStorage;
use crate::entity::attribute::{AttributeMap, AttributeModifier, AttributeModifierOperation};
use crate::fluid::{LavaFluid, get_fluid_state, get_height};
use crate::inventory::equipment::EquipmentSlot;
use crate::physics::{
    COLLISION_EPSILON, CollisionWorld, EntityPhysicsState, MoveResult, MoverType,
    WorldCollisionProvider, move_entity as resolve_entity_movement,
};
use crate::world::game_event_context::GameEventContext;
use crate::world::{ClipBlockShape, ClipFluid, LevelReader, World};
use crate::{enchantment_helper, entity::damage::DamageSource, player::Player};

use entities::ExperienceOrbEntity;

fn nbt_bool(value: bool) -> NbtTag {
    NbtTag::Byte(i8::from(value))
}

fn entity_type_name(entity_type: EntityTypeRef) -> TextComponent {
    TextComponent::translated(TranslatedMessage {
        key: Cow::Owned(format!(
            "entity.{}.{}",
            entity_type.key.namespace, entity_type.key.path
        )),
        fallback: None,
        args: None,
    })
}

fn remove_entity_name_actions(mut component: TextComponent) -> TextComponent {
    fn remove_actions(component: &mut TextComponent) {
        component.interactions.click = None;
        for child in &mut component.children {
            remove_actions(child);
        }
    }

    remove_actions(&mut component);
    component
}

/// Global counter for allocating unique entity IDs.
///
/// Mirrors vanilla's `Entity.ENTITY_COUNTER`. Each new entity increments this
/// counter to get a unique network ID. Starts at 1 (0 is reserved).
static ENTITY_COUNTER: LazyLock<SyncMutex<i32>> = LazyLock::new(|| SyncMutex::new(1));
const MOVEMENT_RECORD_EPSILON: f64 = 1.0e-7;
const NO_PHYSICS_COLLISION_EPSILON: f64 = 1.0e-7;
const IN_WALL_EYE_BOX_HEIGHT: f64 = 1.0e-6;
const WATER_ENTITY_FLOW_SCALE: f64 = 0.014;
const BUBBLE_COLUMN_INSIDE_DOWN_MIN_SPEED: f64 = -0.3;
const BUBBLE_COLUMN_INSIDE_UP_MAX_SPEED: f64 = 0.7;
const BUBBLE_COLUMN_ABOVE_DOWN_MIN_SPEED: f64 = -0.9;
const BUBBLE_COLUMN_ABOVE_UP_MAX_SPEED: f64 = 1.8;
const BUBBLE_COLUMN_DOWN_ACCELERATION: f64 = 0.03;
const BUBBLE_COLUMN_INSIDE_UP_ACCELERATION: f64 = 0.06;
const BUBBLE_COLUMN_ABOVE_UP_ACCELERATION: f64 = 0.1;
const DAMAGE_KNOCKBACK_POWER: f64 = 0.4_f32 as f64;
const KNOCKBACK_DIRECTION_EPSILON_SQ: f64 = 1.0e-5_f32 as f64;
const MOVE_TOWARDS_CLOSEST_SPACE_DIRECTIONS: [Direction; 5] = [
    Direction::North,
    Direction::South,
    Direction::West,
    Direction::East,
    Direction::Up,
];

const fn should_apply_entity_cramming_damage(
    max_cramming: i32,
    pushable_count: usize,
    non_passenger_count: usize,
    random_roll: i32,
) -> bool {
    if max_cramming <= 0 || random_roll != 0 {
        return false;
    }

    let threshold = (max_cramming - 1) as usize;
    pushable_count > threshold && non_passenger_count > threshold
}
const LEASH_SCAN_SIZE: f64 = 32.0;
const LEASH_SCAN_HALF_SIZE: f64 = LEASH_SCAN_SIZE / 2.0;
const SPEED_MODIFIER_POWDER_SNOW_ID: Identifier = Identifier::vanilla_static("powder_snow");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SwimmingEnvironment {
    pub(crate) sprinting: bool,
    pub(crate) passenger: bool,
    pub(crate) in_water: bool,
    pub(crate) under_water: bool,
    pub(crate) block_fluid_is_water: bool,
}

#[must_use]
pub(crate) const fn select_swimming_state(
    currently_swimming: bool,
    env: SwimmingEnvironment,
) -> bool {
    if env.passenger {
        return false;
    }

    if currently_swimming {
        env.sprinting && env.in_water
    } else {
        env.sprinting && env.under_water && env.block_fluid_is_water
    }
}

fn horizontal_distance(vector: DVec3) -> f64 {
    vector.x.hypot(vector.z)
}

const fn world_aabb_center(aabb: WorldAabb) -> DVec3 {
    DVec3::new(
        f64::midpoint(aabb.min_x(), aabb.max_x()),
        f64::midpoint(aabb.min_y(), aabb.max_y()),
        f64::midpoint(aabb.min_z(), aabb.max_z()),
    )
}

fn leash_scan_area(center: DVec3) -> WorldAabb {
    WorldAabb::new(
        center.x - LEASH_SCAN_HALF_SIZE,
        center.y - LEASH_SCAN_HALF_SIZE,
        center.z - LEASH_SCAN_HALF_SIZE,
        center.x + LEASH_SCAN_HALF_SIZE,
        center.y + LEASH_SCAN_HALF_SIZE,
        center.z + LEASH_SCAN_HALF_SIZE,
    )
}

fn transfer_leashables_to_holder(leashables: Vec<SharedEntity>, new_holder: &SharedEntity) -> bool {
    let mut transferred = false;
    for leashable in leashables {
        let Some(mob) = leashable.as_mob() else {
            continue;
        };
        if mob.can_have_a_leash_attached_to(new_holder.as_ref()) {
            let _ = mob.set_leashed_to(new_holder);
            transferred = true;
        }
    }
    transferred
}

fn fall_flying_collision_damage(previous_horizontal_speed: f64, new_horizontal_speed: f64) -> f32 {
    ((previous_horizontal_speed - new_horizontal_speed) * 10.0 - 3.0) as f32
}

fn entity_eye_suffocation_box(eye_pos: DVec3, width: f64) -> WorldAabb {
    let half_width = width * 0.5;
    let half_height = IN_WALL_EYE_BOX_HEIGHT * 0.5;
    WorldAabb::new(
        eye_pos.x - half_width,
        eye_pos.y - half_height,
        eye_pos.z - half_width,
        eye_pos.x + half_width,
        eye_pos.y + half_height,
        eye_pos.z + half_width,
    )
}

fn block_state_suffocates_eye_box(
    state: BlockStateId,
    world: &dyn LevelReader,
    pos: BlockPos,
    eye_box: WorldAabb,
) -> bool {
    if state.is_air() || !state.is_suffocating() {
        return false;
    }

    let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
    behavior
        .get_collision_shape(state, world, pos, BlockCollisionContext::empty())
        .iter()
        .copied()
        .any(|shape| eye_box.intersects(shape.at_block(pos)))
}

const fn fall_flying_free_fall_interval(fall_flying_ticks: i32) -> Option<i32> {
    let check_fall_flying_ticks = fall_flying_ticks.wrapping_add(1);
    if check_fall_flying_ticks % 10 == 0 {
        Some(check_fall_flying_ticks / 10)
    } else {
        None
    }
}

pub(crate) fn equipment_items_to_packet_items(
    items: Vec<(EquipmentSlot, ItemStack)>,
) -> Vec<EquipmentSlotItem> {
    items
        .into_iter()
        .map(|(slot, item_stack)| EquipmentSlotItem { slot, item_stack })
        .collect()
}

fn aabb_contains_any_liquid(world: &Arc<World>, aabb: WorldAabb) -> bool {
    (aabb.min_x().floor() as i32..aabb.max_x().ceil() as i32).any(|x| {
        (aabb.min_y().floor() as i32..aabb.max_y().ceil() as i32).any(|y| {
            (aabb.min_z().floor() as i32..aabb.max_z().ceil() as i32)
                .any(|z| !get_fluid_state(world, BlockPos::new(x, y, z)).is_empty())
        })
    })
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

/// Matches vanilla `LivingEntity.resetForwardDirectionOfRelativePortalPosition`.
#[must_use]
pub(crate) const fn reset_forward_direction_of_relative_portal_position(offsets: DVec3) -> DVec3 {
    DVec3::new(offsets.x, offsets.y, 0.0)
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

mod ageable;
pub(crate) mod ai;
mod animal;
pub mod attribute;
mod base;
mod block_effects;
mod callback;
pub mod damage;
pub mod entities;
mod fluid_contact;
#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/entities.rs"]
mod generated_entities;
mod inside_block_effects;
mod item_based_steering;
mod living_base;
mod manager;
mod mob;
mod movement_sync;
pub mod projectile;
mod registry;
mod shared_flags;
mod spawn;
mod storage;
mod synced_data;
mod ticking;
mod tracker;

use crate::portal::{
    PortalKind, PortalProcessResult, PortalProcessor, PortalTicketTarget, TeleportPostAction,
    TeleportTransition, WorldChangeRequest, portal_shape::PortalShape,
};
pub(crate) use ageable::{AgeableMob, AgeableMobBase};
pub(crate) use animal::{Animal, AnimalBase};
pub use base::{
    DEFAULT_MAX_AIR_SUPPLY, DEFAULT_TICKS_REQUIRED_TO_FREEZE, EntityAmethystStepSound, EntityBase,
    EntityBaseLoad, EntityBaseSaveData, EntityBaseState, EntityFireFreezeState,
    EntityGroundContact, EntityMovement, EntityMovementEmission, EntityMovementFlags,
    EntityMovementProgress, EntityVerticalMovementStateUpdate, MAX_ENTITY_TAGS,
    PendingWorldChangeToken,
};
pub use callback::{
    EntityChunkCallback, EntityLevelCallback, InactiveEntityCallback, NullEntityCallback,
    PlayerEntityCallback, RemovalReason,
};
pub use fluid_contact::EntityFluidContact;
pub use inside_block_effects::{
    InsideBlockEffectCallback, InsideBlockEffectCollector, InsideBlockEffectType,
};
pub(crate) use item_based_steering::{ItemBasedSteering, ItemSteerable};
pub use living_base::{
    ActiveMobEffect, DEATH_DURATION, DEFAULT_SWING_DURATION, LivingEntityBase, LivingRotationState,
    LivingSwingState, LivingTravelInput, MobEffectInstance, MobEffectSyncChange,
    MobEffectSyncPacket,
};
pub use manager::{
    AddEntityError, ChunkEntityLoadResult, EntityLifecycleChanges, EntityMoveError,
    EntityMoveUpdate, EntityOwnership, EntityVisibility, WorldEntityManager,
};
pub(crate) use mob::{Mob, MobBase, PathfinderMob};
pub use movement_sync::{
    EntityMovementSyncPacket, EntityMovementSyncPackets, EntityMovementSyncState,
    EntityMovementSyncUpdate, EntityPositionRotSyncPacket, EntityPositionSyncDecision,
    EntityPositionSyncPacket, EntityPositionSyncSnapshot, EntityPositionSyncState,
    EntityRotationSyncState, EntityVelocitySyncState, POSITION_SYNC_THRESHOLD,
    PackedEntityRotation, ServerEntityMovementSyncState, ServerEntityMovementSyncUpdate,
};
pub use projectile::{
    EntityHitResult, Projectile, ProjectileBase, ProjectileHit, ThrowableItemProjectile,
    ThrowableProjectile, compute_margin,
};
#[cfg(test)]
pub(crate) use registry::init_test_entities;
pub use registry::{ENTITIES, EntityLoadRequest, EntityRegistry, init_entities};
pub(crate) use shared_flags::EntitySharedFlags;
pub(crate) use spawn::{AgeableMobGroupData, EntitySpawnReason, SpawnGroupData};
pub(crate) use storage::EntityStorage;
pub use synced_data::EntitySyncedData;
pub(crate) use ticking::{
    snapshot_old_pos_and_rot_for_tick, tick_vehicle_passengers_with_ticked_if,
};
pub use tracker::{EntityChangeSenders, EntityTracker};

#[cfg(test)]
macro_rules! impl_test_downcast_type {
    ($type:ty) => {
        // SAFETY: A fully qualified test module path plus its local type name is
        // unique within the test process.
        unsafe impl steel_utils::DowncastType for $type {
            const TYPE_KEY: steel_utils::DowncastTypeKey = steel_utils::DowncastTypeKey::new(
                concat!("steel:test/", module_path!(), "/", stringify!($type)),
            );
        }
    };
}

#[cfg(test)]
pub(crate) use impl_test_downcast_type;

/// Type alias for a shared entity reference.
pub type SharedEntity = Arc<dyn Entity>;

/// Type alias for a weak entity reference.
pub type WeakEntity = Weak<dyn Entity>;

/// The point on an entity used by commands that resolve positions or facing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EntityAnchor {
    /// The entity's base position.
    #[default]
    Feet,
    /// The entity's eye position for its current pose.
    Eyes,
}

impl EntityAnchor {
    /// Resolves this anchor against an entity's current position.
    #[must_use]
    pub fn position(self, entity: &dyn Entity) -> DVec3 {
        let position = entity.position();
        match self {
            Self::Feet => position,
            Self::Eyes => DVec3::new(position.x, entity.get_eye_y(), position.z),
        }
    }
}

pub(crate) fn start_riding_entities(
    passenger: &SharedEntity,
    entity_to_ride: &SharedEntity,
) -> bool {
    if !entity_to_ride.could_accept_passenger() {
        return false;
    }

    if !entity_to_ride.entity_type().can_serialize {
        return false;
    }

    if entity_to_ride.id() == passenger.id() {
        return false;
    }

    let mut vehicle_entity = entity_to_ride.vehicle();
    while let Some(vehicle) = vehicle_entity {
        if vehicle.id() == passenger.id() {
            return false;
        }
        vehicle_entity = vehicle.vehicle();
    }

    if !passenger.can_ride(entity_to_ride.as_ref())
        || !entity_to_ride.can_add_passenger(passenger.as_ref())
    {
        return false;
    }

    if passenger.is_passenger() {
        passenger.stop_riding();
    }

    passenger.set_pose(EntityPose::Standing);
    EntityBase::start_riding_relationship(entity_to_ride, passenger);
    // TODO: Emit ENTITY_MOUNT game event and riding advancement trigger once those foundations exist.
    true
}

pub(crate) fn change_entity_world(
    entity: SharedEntity,
    teleport_transition: &TeleportTransition,
) -> Option<SharedEntity> {
    if entity.is_removed() {
        return None;
    }

    let source_world = entity.level()?;
    if source_world.domain() != teleport_transition.target_world.domain() {
        tracing::error!(
            entity_id = entity.id(),
            source_domain = source_world.domain(),
            target_domain = teleport_transition.target_world.domain(),
            "Refusing direct cross-domain entity teleport"
        );
        return None;
    }

    if entity.as_player().is_some() {
        let Some(player) = source_world.players.get_by_entity_id(entity.id()) else {
            tracing::error!(
                entity_id = entity.id(),
                "Refusing player world change for an unregistered player entity"
            );
            return None;
        };
        // Vanilla `ServerPlayer.teleport` keeps the live player/connection identity
        // and sends respawn/player-position packets instead of recreating from entity NBT.
        if !player.change_world_within_domain(teleport_transition) {
            return None;
        }
        return Some(entity);
    }

    change_non_player_entity_world(entity, teleport_transition)
}

fn change_non_player_entity_world(
    entity: SharedEntity,
    teleport_transition: &TeleportTransition,
) -> Option<SharedEntity> {
    if entity.is_removed() {
        return None;
    }

    let Some(source_world) = entity.level() else {
        tracing::warn!(
            entity_id = entity.id(),
            entity_type = ?entity.entity_type().key,
            "Ignoring world change for entity without a live world"
        );
        return None;
    };
    if source_world.domain() != teleport_transition.target_world.domain() {
        tracing::error!(
            entity_id = entity.id(),
            source_domain = source_world.domain(),
            target_domain = teleport_transition.target_world.domain(),
            "Refusing cross-domain non-player entity transition"
        );
        return None;
    }

    entity.set_portal_cooldown(teleport_transition.portal_cooldown);
    if !teleport_transition.as_passenger {
        entity.stop_riding();
    }

    if Arc::ptr_eq(&source_world, &teleport_transition.target_world) {
        teleport_entity_same_world(entity, teleport_transition)
    } else {
        teleport_entity_cross_world(entity, teleport_transition)
    }
}

fn teleport_entity_same_world(
    entity: SharedEntity,
    teleport_transition: &TeleportTransition,
) -> Option<SharedEntity> {
    for passenger in entity.passengers() {
        let passenger_transition =
            passenger_transition(entity.as_ref(), passenger.as_ref(), teleport_transition);
        change_entity_world(passenger, &passenger_transition);
    }

    if let Err(error) = teleport_set_position(
        entity.as_ref(),
        teleport_transition,
        TeleportPositionCommit::Managed,
    ) {
        tracing::warn!(
            entity_id = entity.id(),
            entity_type = ?entity.entity_type().key,
            position = ?teleport_transition.position,
            "Failed to commit same-world portal teleport for entity: {error}"
        );
        return None;
    }

    if !teleport_transition.as_passenger {
        send_teleport_transition_to_riding_players(entity.as_ref(), teleport_transition);
    }
    apply_post_teleport_transition(entity.as_ref(), teleport_transition);
    Some(entity)
}

fn send_teleport_transition_to_riding_players(
    entity: &dyn Entity,
    teleport_transition: &TeleportTransition,
) {
    let controller_id = entity
        .controlling_passenger()
        .map(|controller| controller.id());
    for passenger in indirect_passengers(entity) {
        let Some(player) = passenger.as_player() else {
            continue;
        };
        let packet = if Some(passenger.id()) == controller_id {
            CTeleportEntity::new(
                entity.id(),
                teleport_transition.position,
                teleport_transition.velocity,
                teleport_transition.rotation.0,
                teleport_transition.rotation.1,
                teleport_transition.relatives,
                entity.on_ground(),
            )
        } else {
            let rotation = entity.rotation();
            CTeleportEntity::new(
                entity.id(),
                entity.position(),
                entity.velocity(),
                rotation.0,
                rotation.1,
                RelativeMovement::NONE,
                entity.on_ground(),
            )
        };
        player.send_packet(packet);
    }
}

fn indirect_passengers(entity: &dyn Entity) -> Vec<SharedEntity> {
    fn collect(
        passengers: Vec<SharedEntity>,
        visited: &mut FxHashSet<i32>,
        output: &mut Vec<SharedEntity>,
    ) {
        for passenger in passengers {
            if !visited.insert(passenger.id()) {
                continue;
            }
            output.push(Arc::clone(&passenger));
            collect(passenger.passengers(), visited, output);
        }
    }

    let mut visited = FxHashSet::default();
    visited.insert(entity.id());
    let mut passengers = Vec::new();
    collect(entity.passengers(), &mut visited, &mut passengers);
    passengers
}

fn teleport_entity_cross_world(
    entity: SharedEntity,
    teleport_transition: &TeleportTransition,
) -> Option<SharedEntity> {
    let position = teleport_transition.resolved_position(entity.position());
    let target_chunk = ChunkPos::from_entity_pos(position);
    if !teleport_transition
        .target_world
        .has_full_chunk(target_chunk)
    {
        tracing::warn!(
            entity_id = entity.id(),
            entity_type = ?entity.entity_type().key,
            chunk = ?target_chunk,
            "Ignoring dimension transition for entity because target chunk is not loaded"
        );
        return None;
    }

    let old_passengers = entity.passengers();
    let mut new_passengers = Vec::with_capacity(old_passengers.len());
    for passenger in old_passengers {
        passenger.stop_riding();
        let passenger_transition =
            passenger_transition(entity.as_ref(), passenger.as_ref(), teleport_transition);
        if let Some(new_passenger) = change_entity_world(passenger, &passenger_transition) {
            new_passengers.push(new_passenger);
        }
    }

    let projectile_owner = entity.projectile_owner();
    let Some(persistent) = ChunkStorage::entity_to_dimension_transition_persistent(&entity) else {
        tracing::warn!(
            entity_id = entity.id(),
            entity_type = ?entity.entity_type().key,
            "Failed to serialize entity for dimension transition"
        );
        return None;
    };

    let target_level = Arc::downgrade(&teleport_transition.target_world);
    let mut new_entities = ChunkStorage::persistent_to_entity_tree_at_level(
        &persistent,
        ChunkPos::from_entity_pos(entity.position()),
        &target_level,
    );
    let Some(new_entity) = new_entities.drain(..).next() else {
        tracing::warn!(
            entity_id = entity.id(),
            entity_type = ?entity.entity_type().key,
            "Failed to recreate entity for dimension transition"
        );
        return None;
    };
    if let Some(owner) = &projectile_owner {
        new_entity.restore_owner_reference(owner);
    }

    if let Err(error) = teleport_set_position(
        new_entity.as_ref(),
        teleport_transition,
        TeleportPositionCommit::Local,
    ) {
        tracing::warn!(
            entity_id = entity.id(),
            entity_type = ?entity.entity_type().key,
            position = ?teleport_transition.position,
            "Failed to stage dimension transition position for entity: {error}"
        );
        return None;
    }

    if let Err(error) = teleport_transition
        .target_world
        .try_add_entity(Arc::clone(&new_entity))
    {
        tracing::warn!(
            entity_id = entity.id(),
            new_entity_id = new_entity.id(),
            entity_type = ?new_entity.entity_type().key,
            position = ?new_entity.position(),
            "Failed to register dimension-transition entity: {error}"
        );
        new_entity.set_removed(RemovalReason::Discarded);
        return None;
    }
    if new_entity.entity_type() == &vanilla_entities::ENDER_PEARL
        && let Some(owner) = &projectile_owner
        && let Some(player) = owner.as_player()
    {
        player.register_ender_pearl(&new_entity);
    }

    remove_after_changing_dimensions(entity.as_ref());
    entity.set_removed(RemovalReason::ChangedWorld);
    for new_passenger in new_passengers {
        EntityBase::restore_passenger_relationship(&new_entity, &new_passenger);
    }

    apply_post_teleport_transition(new_entity.as_ref(), teleport_transition);
    Some(new_entity)
}

#[derive(Clone, Copy)]
enum TeleportPositionCommit {
    Managed,
    Local,
}

fn teleport_set_position(
    entity: &dyn Entity,
    teleport_transition: &TeleportTransition,
    commit: TeleportPositionCommit,
) -> Result<(), EntityMoveError> {
    let position = teleport_transition.resolved_position(entity.position());
    let current_rotation = entity.rotation();
    let current_velocity = entity.velocity();
    let rotation = teleport_transition.resolved_rotation(current_rotation);
    let velocity =
        teleport_transition.resolved_velocity(current_velocity, current_rotation, rotation);

    match commit {
        TeleportPositionCommit::Managed => entity.try_set_position(position)?,
        TeleportPositionCommit::Local => entity.base().set_position_local(position),
    }
    entity.set_rotation(rotation);
    if let Some(living) = entity.as_living_entity() {
        living.set_y_head_rot(rotation.0);
    }
    entity.set_old_position_to_current();
    entity.base().set_old_rotation_to_current();
    entity.set_velocity(velocity);
    entity.base().clear_movement_this_tick();
    Ok(())
}

fn passenger_transition(
    vehicle: &dyn Entity,
    passenger: &dyn Entity,
    teleport_transition: &TeleportTransition,
) -> TeleportTransition {
    let rotation = passenger_transition_rotation(
        teleport_transition.rotation,
        teleport_transition.relatives,
        vehicle.rotation(),
        passenger.rotation(),
    );
    let position = passenger_transition_position(
        teleport_transition.position,
        teleport_transition.relatives,
        vehicle.position(),
        passenger.position(),
    );

    TeleportTransition {
        target_world: teleport_transition.target_world.clone(),
        position,
        rotation,
        velocity: teleport_transition.velocity,
        relatives: teleport_transition.relatives,
        portal_cooldown: teleport_transition.portal_cooldown,
        as_passenger: true,
        post_transition: teleport_transition.post_transition.clone(),
    }
}

fn passenger_transition_rotation(
    transition_rotation: (f32, f32),
    relatives: RelativeMovement,
    vehicle_rotation: (f32, f32),
    passenger_rotation: (f32, f32),
) -> (f32, f32) {
    let yaw = transition_rotation.0
        + if relatives.is_y_rot_relative() {
            0.0
        } else {
            passenger_rotation.0 - vehicle_rotation.0
        };
    let pitch = transition_rotation.1
        + if relatives.is_x_rot_relative() {
            0.0
        } else {
            passenger_rotation.1 - vehicle_rotation.1
        };
    (yaw, pitch)
}

fn passenger_transition_position(
    transition_position: DVec3,
    relatives: RelativeMovement,
    vehicle_position: DVec3,
    passenger_position: DVec3,
) -> DVec3 {
    let offset = passenger_position - vehicle_position;
    transition_position
        + DVec3::new(
            if relatives.is_x_relative() {
                0.0
            } else {
                offset.x
            },
            if relatives.is_y_relative() {
                0.0
            } else {
                offset.y
            },
            if relatives.is_z_relative() {
                0.0
            } else {
                offset.z
            },
        )
}

fn apply_post_teleport_transition(entity: &dyn Entity, teleport_transition: &TeleportTransition) {
    for action in teleport_transition.post_transition.actions() {
        match *action {
            TeleportPostAction::PlayPortalSound => {}
            TeleportPostAction::PlacePortalTicket(target) => {
                let Some(world) = entity.level() else {
                    continue;
                };
                let ticket_position = match target {
                    PortalTicketTarget::Destination => BlockPos::from(entity.position()),
                    PortalTicketTarget::Block(pos) => pos,
                };
                world.place_portal_ticket(ticket_position);
            }
        }
    }
}

fn remove_after_changing_dimensions(entity: &dyn Entity) {
    let Some(mob) = entity.as_mob() else {
        return;
    };

    mob.remove_leash();
    for slot in EquipmentSlot::ALL {
        mob.living_base()
            .equipment()
            .lock()
            .set(slot, ItemStack::empty());
    }
}

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

/// Behavior required for vanilla dropped-item merging.
pub trait ItemMergeEntity: Entity {
    /// Returns whether this dropped item can merge with nearby dropped items.
    fn is_mergeable_item_entity(&self) -> bool;

    /// Attempts to merge this dropped item with another mergeable dropped item.
    fn try_merge_item_entity(&self, other: &dyn ItemMergeEntity);

    /// Returns the item stack used by merge comparisons.
    fn item_merge_stack(&self) -> ItemStack;

    /// Returns the vanilla target owner used by merge comparisons.
    fn item_merge_owner(&self) -> Option<Uuid>;

    /// Returns pickup delay and age used when combining item entities.
    fn item_merge_timing(&self) -> (i32, i32);

    /// Applies the merged destination stack and combined timing.
    fn apply_item_merge_destination(&self, stack: ItemStack, pickup_delay: i32, age: i32);

    /// Applies the merged source remainder, or removes the source when empty.
    fn apply_item_merge_source(&self, stack: ItemStack);
}

/// Behavior required for vanilla experience-orb grouping and merging.
pub trait ExperienceOrbMergeEntity: Entity {
    /// Returns whether this orb group can merge with the given vanilla merge key.
    fn can_merge_experience_orb(&self, merge_id: i32, value: i32) -> bool;

    /// Tries to absorb a freshly awarded orb value into this orb group.
    fn try_absorb_awarded_experience_orb(&self, merge_id: i32, value: i32) -> bool;

    /// Tries to merge `other` into this orb group.
    fn try_absorb_experience_orb(
        &self,
        other: &dyn ExperienceOrbMergeEntity,
        merge_id: i32,
        value: i32,
    ) -> bool;

    /// Returns merge count and age for another orb group to absorb.
    fn experience_orb_merge_state(&self) -> (i32, i32);
}

/// Behavior for leash holders that save as a fence-knot block position.
pub trait LeashFenceKnot: Entity {
    /// Returns the fence block this leash knot is attached to.
    fn leash_fence_pos(&self) -> BlockPos;
}

/// Explicit behavior capabilities exposed by a concrete entity implementation.
///
/// This mirrors vanilla `instanceof` branches without relying on `Any` or
/// introducing a shared implementation hierarchy. Concrete entities should
/// expose these through the `#[entity_impl(class(...), interfaces(...))]`
/// macro so missing trait impls fail at compile time.
pub struct EntityCapabilities<'a> {
    player: Option<&'a Player>,
    living: Option<&'a dyn LivingEntity>,
    mob: Option<&'a dyn Mob>,
    pathfinder_mob: Option<&'a dyn PathfinderMob>,
    animal: Option<&'a dyn Animal>,
    item_steerable: Option<&'a dyn ItemSteerable>,
    item_merge_entity: Option<&'a dyn ItemMergeEntity>,
    experience_orb_merge_entity: Option<&'a dyn ExperienceOrbMergeEntity>,
    leash_fence_knot: Option<&'a dyn LeashFenceKnot>,
}

impl<'a> EntityCapabilities<'a> {
    /// Returns an entity capability set with no specialized behavior.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            player: None,
            living: None,
            mob: None,
            pathfinder_mob: None,
            animal: None,
            item_steerable: None,
            item_merge_entity: None,
            experience_orb_merge_entity: None,
            leash_fence_knot: None,
        }
    }

    /// Exposes player-specific behavior for this entity.
    #[must_use]
    pub const fn with_player(mut self, player: &'a Player) -> Self {
        self.player = Some(player);
        self
    }

    /// Exposes living-entity behavior for this entity.
    #[must_use]
    pub const fn with_living(mut self, living: &'a dyn LivingEntity) -> Self {
        self.living = Some(living);
        self
    }

    /// Exposes mob behavior for this entity.
    #[must_use]
    pub const fn with_mob(mut self, mob: &'a dyn Mob) -> Self {
        self.mob = Some(mob);
        self
    }

    /// Exposes pathfinder-mob behavior for this entity.
    #[must_use]
    pub const fn with_pathfinder_mob(mut self, pathfinder_mob: &'a dyn PathfinderMob) -> Self {
        self.pathfinder_mob = Some(pathfinder_mob);
        self
    }

    /// Exposes animal behavior for this entity.
    #[must_use]
    pub const fn with_animal(mut self, animal: &'a dyn Animal) -> Self {
        self.animal = Some(animal);
        self
    }

    /// Exposes item-steerable behavior for this entity.
    #[must_use]
    pub const fn with_item_steerable(mut self, item_steerable: &'a dyn ItemSteerable) -> Self {
        self.item_steerable = Some(item_steerable);
        self
    }

    /// Exposes dropped-item merge behavior for this entity.
    #[must_use]
    pub const fn with_item_merge_entity(
        mut self,
        item_merge_entity: &'a dyn ItemMergeEntity,
    ) -> Self {
        self.item_merge_entity = Some(item_merge_entity);
        self
    }

    /// Exposes experience-orb merge behavior for this entity.
    #[must_use]
    pub const fn with_experience_orb_merge_entity(
        mut self,
        experience_orb_merge_entity: &'a dyn ExperienceOrbMergeEntity,
    ) -> Self {
        self.experience_orb_merge_entity = Some(experience_orb_merge_entity);
        self
    }

    /// Exposes leash fence-knot behavior for this entity.
    #[must_use]
    pub const fn with_leash_fence_knot(mut self, leash_fence_knot: &'a dyn LeashFenceKnot) -> Self {
        self.leash_fence_knot = Some(leash_fence_knot);
        self
    }
}

/// A trait for entities.
///
/// This trait provides the core functionality for entities.
/// It's based on Minecraft's `Entity` class.
/// Concrete implementations must also claim a unique [`steel_utils::DowncastTypeKey`]
/// through [`steel_utils::DowncastType`].
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
pub trait Entity: EntityEventSource + ErasedType + Send + Sync {
    /// Returns a reference to the entity's shared vanilla base fields.
    fn base(&self) -> &EntityBase;

    /// Gets the entity type containing tracking range, dimensions, etc.
    fn entity_type(&self) -> EntityTypeRef;

    /// Returns whether this entity ignores chunk ticking visibility.
    ///
    /// Mirrors vanilla `Entity.isAlwaysTicking`.
    fn is_always_ticking(&self) -> bool {
        false
    }

    /// Returns whether this entity should be broadcast to the given player.
    ///
    /// Mirrors vanilla `Entity.broadcastToPlayer`. Most entities are always
    /// broadcastable; players override this for spectator visibility rules.
    fn broadcast_to_player(&self, _player: &Player) -> bool {
        true
    }

    /// Returns specialized behavior capabilities exposed by this entity.
    fn capabilities(&self) -> EntityCapabilities<'_> {
        EntityCapabilities::none()
    }

    /// Gets the entity's unique network ID (session-local).
    fn id(&self) -> i32 {
        self.base().id()
    }

    /// Gets the UUID of the entity (persistent identifier).
    fn uuid(&self) -> Uuid {
        self.base().uuid()
    }

    /// Returns this entity's vanilla scoreboard holder name.
    ///
    /// Non-player entities use their UUID string. Players override this with
    /// their game profile name.
    fn scoreboard_name(&self) -> String {
        self.uuid().to_string()
    }

    /// Returns this entity's vanilla name component before team decoration.
    fn name(&self) -> TextComponent {
        self.custom_name().map_or_else(
            || entity_type_name(self.entity_type()),
            remove_entity_name_actions,
        )
    }

    /// Returns this entity's vanilla display name.
    fn display_name(&self) -> TextComponent {
        let name = self.name();
        name.clone()
            .hover_event(HoverEvent::show_entity(
                self.entity_type().key.to_string(),
                self.uuid(),
                Some(name),
            ))
            .insertion(self.uuid().to_string())
    }

    /// Returns this entity's plain vanilla name.
    ///
    /// Custom names are resolved as text components; otherwise entities use
    /// their translated type description. Players override this with their
    /// game profile name.
    fn plain_text_name(&self) -> String {
        self.name().to_plain(&DisplayResolutor)
    }

    /// Returns the vanilla-shaped entity NBT used by command predicates.
    ///
    /// Mirrors `NbtPredicate.getEntityTagToCompare`: base and type-specific
    /// `Entity.saveWithoutId` data, passengers, and a player's selected item.
    fn nbt_for_data_compare(&self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        let position = self.vehicle().map_or_else(
            || self.position(),
            |vehicle| {
                DVec3::new(
                    vehicle.position().x,
                    self.position().y,
                    vehicle.position().z,
                )
            },
        );
        let velocity = self.velocity();
        let (yaw, pitch) = self.rotation();
        let fire_freeze = self.fire_freeze_state();

        nbt.insert(
            "Pos",
            NbtList::Double(vec![position.x, position.y, position.z]),
        );
        nbt.insert(
            "Motion",
            NbtList::Double(vec![velocity.x, velocity.y, velocity.z]),
        );
        nbt.insert("Rotation", NbtList::Float(vec![yaw, pitch]));
        nbt.insert("fall_distance", self.fall_distance());
        nbt.insert(
            "Fire",
            NbtTag::Short(fire_freeze.remaining_fire_ticks() as i16),
        );
        nbt.insert("Air", NbtTag::Short(self.air_supply() as i16));
        nbt.insert("OnGround", nbt_bool(self.on_ground()));
        nbt.insert("Invulnerable", nbt_bool(self.is_invulnerable()));
        nbt.insert("PortalCooldown", self.portal_cooldown());
        nbt.insert(
            "UUID",
            NbtTag::IntArray(self.uuid().to_int_array().to_vec()),
        );

        if let Some(custom_name) = self.custom_name() {
            nbt.insert("CustomName", custom_name.to_nbt_tag());
        }
        if self.is_custom_name_visible() {
            nbt.insert("CustomNameVisible", nbt_bool(true));
        }
        if self.is_silent() {
            nbt.insert("Silent", nbt_bool(true));
        }
        if self.is_no_gravity() {
            nbt.insert("NoGravity", nbt_bool(true));
        }
        if self.has_glowing_tag() {
            nbt.insert("Glowing", nbt_bool(true));
        }
        if fire_freeze.ticks_frozen() > 0 {
            nbt.insert("TicksFrozen", fire_freeze.ticks_frozen());
        }
        if fire_freeze.has_visual_fire() {
            nbt.insert("HasVisualFire", nbt_bool(true));
        }

        let tags = self.tags();
        if !tags.is_empty() {
            nbt.insert("Tags", NbtList::from(tags));
        }
        let custom_data = self.custom_data();
        if !custom_data.is_empty() {
            nbt.insert("data", NbtTag::Compound(custom_data));
        }

        if let Some(living) = self.as_living_entity() {
            living.save_command_nbt(&mut nbt);
        }
        self.save_additional(&mut nbt);

        if let Some(player) = self.as_player() {
            player.save_command_nbt(&mut nbt);
        }

        let passengers = self
            .passengers()
            .into_iter()
            .filter_map(|passenger| passenger.nbt_for_passenger_save())
            .collect::<Vec<_>>();
        if !passengers.is_empty() {
            nbt.insert("Passengers", NbtList::Compound(passengers));
        }

        if let Some(player) = self.as_player() {
            let inventory = player.inventory.lock();
            let selected_item = inventory.get_selected_item();
            if !selected_item.is_empty() {
                nbt.insert("SelectedItem", selected_item.to_nbt_tag_ref());
            }
        }

        nbt
    }

    /// Returns passenger-save NBT including the entity type id.
    fn nbt_for_passenger_save(&self) -> Option<NbtCompound> {
        if !self.removal_reason().is_none_or(RemovalReason::should_save)
            || !self.entity_type().can_serialize
        {
            return None;
        }

        let mut nbt = self.nbt_for_data_compare();
        nbt.insert("id", self.entity_type().key.to_string());
        Some(nbt)
    }

    /// Gets the entity's current position.
    fn position(&self) -> DVec3 {
        self.base().position()
    }

    /// Gets the position to send in this entity's add-entity packet.
    ///
    /// Mirrors vanilla `Entity.getAddEntityPacket()` overloads. Most entities
    /// spawn at their current position; block-attached entities can override
    /// this with the backing block position used by the vanilla packet.
    fn spawn_position(&self) -> DVec3 {
        self.position()
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

        let target_box = self.bounding_box().translate(delta);
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

    /// Returns whether this entity can be attacked.
    ///
    /// Mirrors vanilla `Entity.isAttackable`. Concrete entities that override
    /// vanilla to reject player attacks should override this method.
    fn attackable(&self) -> bool {
        true
    }

    /// Returns whether this entity handles and consumes an attack before normal damage.
    ///
    /// Mirrors vanilla `Entity.skipAttackInteraction`.
    fn skip_attack_interaction(&self, _source: &dyn Entity) -> bool {
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

    /// Applies vanilla `Entity.onAboveBubbleColumn`.
    fn on_above_bubble_column(&self, drag_down: bool, _pos: BlockPos) {
        if self.is_flying_player() {
            return;
        }

        let velocity = self.velocity();
        let y = if drag_down {
            (velocity.y - BUBBLE_COLUMN_DOWN_ACCELERATION).max(BUBBLE_COLUMN_ABOVE_DOWN_MIN_SPEED)
        } else {
            (velocity.y + BUBBLE_COLUMN_ABOVE_UP_ACCELERATION).min(BUBBLE_COLUMN_ABOVE_UP_MAX_SPEED)
        };
        self.set_velocity(DVec3::new(velocity.x, y, velocity.z));
    }

    /// Applies vanilla `Entity.onInsideBubbleColumn`.
    fn on_inside_bubble_column(&self, drag_down: bool) {
        if self.is_flying_player() {
            return;
        }

        let velocity = self.velocity();
        let y = if drag_down {
            (velocity.y - BUBBLE_COLUMN_DOWN_ACCELERATION).max(BUBBLE_COLUMN_INSIDE_DOWN_MIN_SPEED)
        } else {
            (velocity.y + BUBBLE_COLUMN_INSIDE_UP_ACCELERATION)
                .min(BUBBLE_COLUMN_INSIDE_UP_MAX_SPEED)
        };
        self.set_velocity(DVec3::new(velocity.x, y, velocity.z));
        self.reset_fall_distance();
    }

    /// Returns whether this entity is invisible to normal entity selectors.
    ///
    /// Mirrors vanilla `Entity.isSpectator`. Base entities are never spectators;
    /// players override this from their game mode.
    fn is_spectator(&self) -> bool {
        false
    }

    /// Returns whether vanilla lets this entity interact with its loaded level.
    fn can_interact_with_level(&self) -> bool {
        self.is_alive() && !self.is_removed() && !self.is_spectator()
    }

    /// Returns vanilla `Entity.isInvisible()`.
    fn is_invisible(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_base_invisible_flag)
    }

    /// Returns vanilla `Entity.isDiscrete()`.
    fn is_discrete(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_shift_key_down)
    }

    /// Returns whether this entity is allied to `other`.
    fn is_allied_to(&self, _other: &dyn Entity) -> bool {
        false
    }

    /// Returns whether this entity is a marker armor stand.
    fn is_marker_armor_stand(&self) -> bool {
        false
    }

    /// Returns whether this entity is a tameable animal owned by `owner`.
    fn is_tame_owned_by(&self, _owner: &dyn LivingEntity) -> bool {
        false
    }

    /// Returns the vanilla `Projectile` owner UUID when this entity exposes one.
    fn projectile_owner_uuid(&self) -> Option<Uuid> {
        None
    }

    /// Returns the live vanilla `Projectile` owner when this entity exposes one.
    fn projectile_owner(&self) -> Option<SharedEntity> {
        None
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

    /// Returns the vehicle this entity directly controls, if any.
    ///
    /// Mirrors vanilla `Entity.getControlledVehicle`.
    fn controlled_vehicle(&self) -> Option<SharedEntity> {
        let vehicle = self.vehicle()?;
        let controlled_by_self = vehicle
            .controlling_passenger()
            .is_some_and(|passenger| passenger.id() == self.id());
        controlled_by_self.then_some(vehicle)
    }

    /// Returns whether this entity is riding another entity.
    ///
    /// Mirrors vanilla `Entity.isPassenger`.
    fn is_passenger(&self) -> bool {
        self.vehicle().is_some()
    }

    /// Returns whether vanilla allows this entity to start riding `vehicle`.
    ///
    /// Mirrors vanilla `Entity.canRide`.
    fn can_ride(&self, _vehicle: &dyn Entity) -> bool {
        !self.is_discrete() && self.base().boarding_cooldown() <= 0
    }

    /// Stops riding the current vehicle, if any.
    ///
    /// Mirrors vanilla `Entity.stopRiding`.
    fn stop_riding(&self) {
        self.base().stop_riding();
    }

    /// Starts riding `entity_to_ride` if vanilla boarding rules allow it.
    ///
    /// Mirrors vanilla `Entity.startRiding(Entity)`.
    fn start_riding(&self, entity_to_ride: &SharedEntity) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let Some(passenger) = world.get_entity_by_id(self.id()) else {
            return false;
        };
        start_riding_entities(&passenger, entity_to_ride)
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

    /// Returns whether this entity can control a vehicle it is riding.
    ///
    /// Mirrors vanilla `Entity.canControlVehicle`.
    fn can_control_vehicle(&self) -> bool {
        !REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::NON_CONTROLLING_RIDER)
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

    /// Returns vanilla `Entity.dismountsUnderwater`.
    fn dismounts_underwater(&self) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::DISMOUNTS_UNDERWATER)
    }

    /// Returns whether `passenger` is a direct passenger of this entity.
    ///
    /// Mirrors vanilla `Entity.hasPassenger(Entity)`.
    fn has_passenger(&self, passenger: &dyn Entity) -> bool {
        self.base().has_passenger_id(passenger.id())
    }

    /// Returns whether this entity can accept `passenger` as a direct passenger.
    ///
    /// Mirrors vanilla `Entity.canAddPassenger`.
    fn can_add_passenger(&self, _passenger: &dyn Entity) -> bool {
        self.passengers().is_empty()
    }

    /// Returns whether this entity can accept any passenger.
    ///
    /// Mirrors vanilla `Entity.couldAcceptPassenger`.
    fn could_accept_passenger(&self) -> bool {
        true
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

    /// Returns vanilla `Entity.getRelativePortalPosition`.
    fn get_relative_portal_position(&self, axis: Axis, portal_area: FoundRectangle) -> DVec3 {
        let offsets = PortalShape::get_relative_position(
            portal_area,
            axis,
            self.position(),
            self.dimensions_for_pose(self.pose()),
        );
        if self.as_living_entity().is_some() {
            reset_forward_direction_of_relative_portal_position(offsets)
        } else {
            offsets
        }
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

    /// Runs vanilla `Entity.baseTick` pieces Steel currently implements.
    ///
    /// This intentionally stays separate from `tick()` because several vanilla
    /// subclasses override tick without calling `super.tick()`.
    fn base_tick(&self) {
        self.entity_base_tick();
    }

    /// Runs vanilla `Entity.handlePortal` behavior currently implemented by Steel.
    fn handle_portal(&self) {
        self.base().process_portal_cooldown();
        let Some(world) = self.level() else {
            return;
        };
        let Some(process) = self.base().portal_process() else {
            return;
        };

        let player_invulnerable = self
            .as_player()
            .map(|player| player.abilities.lock().invulnerable);
        let transition_time = process
            .portal()
            .transition_time_for_player_state(&world, player_invulnerable);
        match self
            .base()
            .process_portal_teleportation(self.can_use_portal(false), transition_time)
        {
            Some(PortalProcessResult::Ready) => {
                let Some(pending_token) = self.begin_pending_world_change() else {
                    return;
                };
                let Some(entity) = world.get_entity_by_id(self.id()) else {
                    self.finish_pending_world_change(pending_token);
                    return;
                };
                self.reset_portal_cooldown();
                world.queue_world_change(
                    entity,
                    WorldChangeRequest::Portal {
                        portal: process.portal(),
                        source_world: world.clone(),
                        portal_pos: process.entry_position(),
                        pending_token,
                    },
                );
            }
            Some(PortalProcessResult::Waiting)
                if self
                    .base()
                    .portal_process()
                    .is_some_and(PortalProcessor::has_expired) =>
            {
                self.base().clear_portal_process();
            }
            Some(PortalProcessResult::Waiting) | None => {}
        }
    }

    /// Runs only vanilla `Entity.baseTick` behavior.
    ///
    /// Subtype base-tick chains call this from their owner trait instead of
    /// discovering subtype behavior through runtime capabilities.
    fn entity_base_tick(&self) {
        self.base().advance_base_tick_state();
        self.handle_portal();
        self.base().advance_powder_snow_contact_for_base_tick();
        self.refresh_fluid_contact_for_base_tick();
        self.update_swimming();
        self.base().reset_fall_distance_in_water();
        if self
            .base()
            .advance_fire_tick(self.fire_immune(), self.is_in_lava())
            && let Some(world) = self.level()
        {
            self.hurt(
                &world,
                &DamageSource::environment(&vanilla_damage_types::ON_FIRE),
                1.0,
            );
        }
        self.base().dampen_fall_distance_in_lava();
        self.check_below_world();
        self.sync_base_fire_freeze_entity_data();
        // Vanilla checks `this instanceof Leashable` inside `Entity.baseTick`.
        if let Some(mob) = self.as_mob() {
            mob.tick_leash();
        }
        // TODO: Add remaining vanilla baseTick pieces: portal and sprint particles.
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

    /// Runs vanilla pre-tick despawn checks.
    fn check_despawn(&self) {}

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

    /// Updates synchronized entity data just before tracker sync.
    ///
    /// Mirrors vanilla `Entity.updateDataBeforeSync`.
    fn update_data_before_sync(&self) {}

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

    /// Drains dirty mob-effect packet changes for vanilla recipients.
    fn drain_dirty_mob_effects(&self) -> Vec<MobEffectSyncChange> {
        Vec::new()
    }

    /// Packs non-empty equipment slots for initial spawn pairing.
    fn pack_all_equipment(&self) -> Vec<EquipmentSlotItem> {
        Vec::new()
    }

    /// Drains equipment slots that changed since the last tracker sync.
    fn drain_dirty_equipment(&self) -> Vec<EquipmentSlotItem> {
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

    /// Marks this entity as waiting for a prepared world change.
    fn begin_pending_world_change(&self) -> Option<PendingWorldChangeToken> {
        self.base().begin_pending_world_change()
    }

    /// Clears a pending world change if it still matches the provided token.
    fn finish_pending_world_change(&self, token: PendingWorldChangeToken) -> bool {
        self.base().finish_pending_world_change(token)
    }

    /// Returns true while this entity is waiting for a prepared world change.
    fn is_world_change_pending(&self) -> bool {
        self.base().is_world_change_pending()
    }

    /// Returns true if the given world-change token is still pending.
    fn is_world_change_token_pending(&self, token: PendingWorldChangeToken) -> bool {
        self.base().is_world_change_token_pending(token)
    }

    /// Returns whether this entity may enter a portal.
    ///
    /// Mirrors vanilla `Entity.canUsePortal`, including `LivingEntity` sleeping
    /// suppression through exposed behavior capabilities.
    fn can_use_portal(&self, ignore_passenger: bool) -> bool {
        let entity_type = self.entity_type();
        if entity_type == &vanilla_entities::FISHING_BOBBER
            || entity_type == &vanilla_entities::ENDER_DRAGON
            || entity_type == &vanilla_entities::WITHER
        {
            return false;
        }

        (ignore_passenger || !self.is_passenger())
            && self.is_alive()
            && !self
                .as_living_entity()
                .is_some_and(LivingEntity::is_sleeping)
    }

    /// Returns vanilla's dimension-changing portal cooldown delay in ticks.
    ///
    /// Mirrors vanilla `getDimensionChangingDelay` overrides for players,
    /// vehicle entities, projectiles, and base entities with a player passenger.
    fn dimension_changing_delay(&self) -> i32 {
        let entity_type = self.entity_type();
        if self.as_player().is_some() || entity_type.is_vehicle_entity {
            return 10;
        }
        if entity_type.is_projectile {
            return 2;
        }
        if let Some(first_passenger) = self.first_passenger()
            && first_passenger.as_player().is_some()
        {
            return first_passenger.dimension_changing_delay();
        }
        300
    }

    /// Returns why this entity was removed, if it has been removed.
    fn removal_reason(&self) -> Option<RemovalReason> {
        self.base().removal_reason()
    }

    /// Marks the entity as removed with the given reason.
    fn set_removed(&self, reason: RemovalReason) {
        self.base().set_removed(reason);
    }

    /// Emits a vanilla game event from this entity's exact position.
    fn game_event(&self, event: GameEventRef) {
        let Some(world) = self.level() else {
            return;
        };
        world.game_event_at(
            event,
            self.position(),
            &GameEventContext::new(Some(self.as_entity_event_source()), None),
        );
    }

    /// Kills this entity using vanilla's living/non-living class split.
    ///
    /// `world` is vanilla's explicit `ServerLevel` argument. Living entities
    /// use it for damage processing, while death game events use the entity's
    /// attached world.
    fn kill(&self, world: &World) {
        if self.is_living_entity() {
            self.hurt(
                world,
                &DamageSource::environment(&vanilla_damage_types::GENERIC_KILL),
                f32::MAX,
            );
            return;
        }

        self.set_removed(RemovalReason::Killed);
        self.game_event(&vanilla_game_events::ENTITY_DIE);
    }

    /// Caches a live owner reference after restoring persisted owner-linked
    /// entities. Most entities do not store owner references.
    fn restore_owner_reference(&self, _owner: &SharedEntity) {}

    /// Sets the level callback for lifecycle events (movement, removal).
    fn set_level_callback(&self, callback: Arc<dyn EntityLevelCallback>) {
        self.base().set_level_callback(callback);
    }

    /// Called by leashables while this entity is their live leash holder.
    fn notify_leash_holder(&self, _leashable: &dyn Entity) {}

    /// Called when a leashable stops using this entity as its live leash holder.
    fn notify_leashee_removed(&self, _leashable: &dyn Entity) {}

    /// Called when a player touches this entity during nearby pickup processing.
    fn player_touch(self: Arc<Self>, _player: &Arc<Player>) {}

    /// Finds leashable mobs in vanilla's nearby leash scan whose holder is this entity.
    fn leashables_leashed_to(&self) -> Vec<SharedEntity> {
        self.leashables_leashed_to_holder_in_area(self.as_entity_event_source())
    }

    /// Finds leashable mobs in this entity's nearby leash scan whose holder is `holder`.
    fn leashables_leashed_to_holder_in_area(&self, holder: &dyn Entity) -> Vec<SharedEntity> {
        let Some(world) = self.level() else {
            return Vec::new();
        };
        let holder_id = holder.id();
        let scan_area = leash_scan_area(world_aabb_center(self.bounding_box()));
        world.get_entities_in_aabb_matching(&scan_area, |entity| {
            entity.as_mob().is_some_and(|mob| {
                mob.leash_holder()
                    .is_some_and(|holder| holder.id() == holder_id)
            })
        })
    }

    /// Transfers leashables currently held by `old_holder` to this entity.
    fn transfer_leashables_from_holder(&self, old_holder: &dyn Entity) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let Some(new_holder) = world.get_entity_by_id(self.id()) else {
            return false;
        };

        transfer_leashables_to_holder(
            self.leashables_leashed_to_holder_in_area(old_holder),
            &new_holder,
        )
    }

    /// Drops this entity's leash and all leashables attached to it.
    fn drop_all_leash_connections(&self, player: Option<&Player>) -> bool {
        let leashables = self.leashables_leashed_to();
        let mut dropped = !leashables.is_empty();

        if let Some(mob) = self.as_mob()
            && mob.is_leashed()
        {
            mob.drop_leash();
            dropped = true;
        }

        for leashable in leashables {
            if let Some(mob) = leashable.as_mob() {
                mob.drop_leash();
            }
        }

        if !dropped {
            return false;
        }

        if let Some(world) = self.level() {
            let source_entity = player.map(|player| player as &dyn Entity);
            world.game_event(
                &vanilla_game_events::SHEAR,
                self.block_position(),
                &GameEventContext::new(source_entity, None),
            );
        }
        true
    }

    /// Shears off all leash connections reachable from this entity.
    fn shear_off_all_leash_connections(&self, player: Option<&Player>) -> bool {
        if !self.drop_all_leash_connections(player) {
            return false;
        }

        if let Some(world) = self.level() {
            let sound_source = player.map_or_else(|| self.sound_source(), Entity::sound_source);
            world.play_sound(
                &sound_events::ITEM_SHEARS_SNIP,
                sound_source,
                self.block_position(),
                1.0,
                1.0,
                None,
            );
        }
        true
    }

    /// Runs vanilla `Entity.attemptToShearEquipment`.
    fn attempt_to_shear_equipment(&self, player: &Player, hand: InteractionHand) -> bool {
        let Some(mob) = self.as_mob() else {
            return false;
        };
        if !mob.can_shear_equipment(player) || player.is_secondary_use_active() {
            return false;
        }

        let has_infinite_materials = player.has_infinite_materials();
        for slot in EquipmentSlot::ALL {
            let sheared = {
                let mut equipment = mob.living_base().equipment().lock();
                let item_stack = equipment.get_ref(slot);
                let Some(equippable) = item_stack.get_equippable() else {
                    continue;
                };
                if !equippable.can_be_sheared
                    || !has_infinite_materials
                        && item_stack
                            .has_enchantment_effect(EnchantmentEffectComponent::PreventArmorChange)
                {
                    continue;
                }

                let shearing_sound = equippable.shearing_sound.registry_ref();
                (equipment.take(slot), shearing_sound)
            };
            let (item_stack, shearing_sound) = sheared;
            if item_stack.is_empty() {
                continue;
            }

            player
                .inventory
                .lock()
                .hurt_item_in_hand(hand, 1, has_infinite_materials);
            mob.refresh_equipment_attribute_modifiers(slot);
            mob.set_guaranteed_drop(slot);
            mob.set_persistence_required();

            if let Some(world) = self.level() {
                world.game_event(
                    &vanilla_game_events::SHEAR,
                    self.block_position(),
                    &GameEventContext::new(Some(player), None),
                );
            }
            if let Some(shearing_sound) = shearing_sound {
                self.play_sound(shearing_sound, 1.0, 1.0);
            }

            let dimensions = self.base().dimensions();
            let spawn_offset = dimensions
                .attachments
                .get_average(EntityAttachment::Passenger, dimensions);
            let _ = self.spawn_at_location_with_offset(item_stack, spawn_offset);
            // TODO: Trigger PLAYER_SHEARED_EQUIPMENT once advancement criteria exist.
            return true;
        }

        false
    }

    /// Handles vanilla entity right-click interaction.
    fn interact(
        &self,
        player: &Player,
        hand: InteractionHand,
        location: DVec3,
    ) -> InteractionResult {
        self.interact_entity(player, hand, location)
    }

    /// Handles shared vanilla `Entity.interact` behavior.
    fn interact_entity(
        &self,
        player: &Player,
        hand: InteractionHand,
        _location: DVec3,
    ) -> InteractionResult {
        let Some(mob) = self.as_mob() else {
            return InteractionResult::Pass;
        };
        let is_alive = self.as_living_entity().is_none_or(LivingEntity::is_alive);
        if !is_alive {
            return InteractionResult::Pass;
        }

        if player.is_secondary_use_active()
            && mob.can_be_leashed()
            && self
                .as_living_entity()
                .is_none_or(|living| !LivingEntity::is_baby(living))
            && self.transfer_leashables_from_holder(player)
        {
            if let Some(world) = self.level() {
                world.game_event(
                    &vanilla_game_events::ENTITY_ACTION,
                    self.block_position(),
                    &GameEventContext::new(Some(player), None),
                );
            }
            self.play_sound(&sound_events::ITEM_LEAD_TIED, 1.0, 1.0);
            return InteractionResult::SuccessServer;
        }

        let holding_shears = {
            let inventory = player.inventory.lock();
            inventory
                .get_item_in_hand(hand)
                .is(&vanilla_items::ITEMS.shears)
        };
        if holding_shears && self.shear_off_all_leash_connections(Some(player)) {
            let has_infinite_materials = player.has_infinite_materials();
            player
                .inventory
                .lock()
                .hurt_item_in_hand(hand, 1, has_infinite_materials);
            return InteractionResult::Success;
        }
        if holding_shears && self.attempt_to_shear_equipment(player, hand) {
            return InteractionResult::Success;
        }

        if let Some(holder) = mob.leash_holder() {
            if holder.id() == player.id() {
                if player.has_infinite_materials() {
                    mob.remove_leash();
                } else {
                    mob.drop_leash();
                }

                if let Some(world) = self.level() {
                    world.game_event(
                        &vanilla_game_events::ENTITY_INTERACT,
                        self.block_position(),
                        &GameEventContext::new(Some(player), None),
                    );
                }
                self.play_sound(&sound_events::ITEM_LEAD_UNTIED, 1.0, 1.0);
                return InteractionResult::Success;
            }

            if holder.as_player().is_some() {
                return InteractionResult::Pass;
            }
        }

        let holding_lead = {
            let inventory = player.inventory.lock();
            inventory
                .get_item_in_hand(hand)
                .is(&vanilla_items::ITEMS.lead)
        };
        if !holding_lead || !mob.can_have_a_leash_attached_to(player) {
            return InteractionResult::Pass;
        }

        let Some(world) = self.level() else {
            return InteractionResult::Pass;
        };
        let Some(player_entity) = world.get_entity_by_id(player.id()) else {
            return InteractionResult::Pass;
        };

        if mob.is_leashed() {
            mob.drop_leash();
        }
        if !mob.set_leashed_to(&player_entity) {
            return InteractionResult::Pass;
        }

        self.play_sound(&sound_events::ITEM_LEAD_TIED, 1.0, 1.0);
        player.inventory.lock().shrink_item_in_hand(hand, 1);
        InteractionResult::SuccessServer
    }

    /// Returns true for entities that implement vanilla living-entity behavior.
    fn is_living_entity(&self) -> bool {
        self.as_living_entity().is_some()
    }

    /// Returns this entity as a living entity when it has living behavior.
    ///
    /// Mirrors vanilla's frequent `instanceof LivingEntity` branches without
    /// requiring core code to downcast through `Any`.
    fn as_living_entity(&self) -> Option<&dyn LivingEntity> {
        self.capabilities().living
    }

    /// Returns this entity as a player when it is the concrete server player.
    ///
    /// Mirrors vanilla player-only branches without requiring core code to
    /// downcast through `Any`.
    fn as_player(&self) -> Option<&Player> {
        self.capabilities().player
    }

    /// Returns true for mobs with pathfinding navigation.
    fn is_pathfinder_mob(&self) -> bool {
        self.as_pathfinder_mob().is_some()
    }

    /// Returns this entity as a pathfinder mob when it has pathfinding behavior.
    ///
    /// Mirrors vanilla's frequent `instanceof PathfinderMob` branches without
    /// requiring core code to downcast through `Any`.
    fn as_pathfinder_mob(&self) -> Option<&dyn PathfinderMob> {
        self.capabilities().pathfinder_mob
    }

    /// Returns true for entities that implement vanilla mob behavior.
    fn is_mob(&self) -> bool {
        self.as_mob().is_some()
    }

    /// Returns this entity as a mob when it has mob behavior.
    ///
    /// Mirrors vanilla's frequent `instanceof Mob` branches without requiring
    /// core code to downcast through `Any`.
    fn as_mob(&self) -> Option<&dyn Mob> {
        self.capabilities().mob
    }

    /// Returns true for entities that implement vanilla animal behavior.
    fn is_animal(&self) -> bool {
        self.as_animal().is_some()
    }

    /// Returns this entity as an animal when it has animal behavior.
    ///
    /// Mirrors vanilla's frequent `instanceof Animal` branches without
    /// requiring core code to downcast through `Any`.
    fn as_animal(&self) -> Option<&dyn Animal> {
        self.capabilities().animal
    }

    /// Returns true for entities that implement vanilla item-steered boosts.
    fn is_item_steerable(&self) -> bool {
        self.as_item_steerable().is_some()
    }

    /// Returns this entity as item steerable when it has item-steering behavior.
    ///
    /// Mirrors vanilla's `instanceof ItemSteerable` branches without requiring
    /// core code to downcast through `Any`.
    fn as_item_steerable(&self) -> Option<&dyn ItemSteerable> {
        self.capabilities().item_steerable
    }

    /// Returns dropped-item merge behavior when this entity exposes it.
    fn as_item_merge_entity(&self) -> Option<&dyn ItemMergeEntity> {
        self.capabilities().item_merge_entity
    }

    /// Returns experience-orb merge behavior when this entity exposes it.
    fn as_experience_orb_merge_entity(&self) -> Option<&dyn ExperienceOrbMergeEntity> {
        self.capabilities().experience_orb_merge_entity
    }

    /// Returns leash fence-knot behavior when this entity exposes it.
    fn as_leash_fence_knot(&self) -> Option<&dyn LeashFenceKnot> {
        self.capabilities().leash_fence_knot
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

    /// Updates the vanilla swimming shared flag.
    ///
    /// Mirrors vanilla `Entity.updateSwimming`.
    fn update_swimming(&self) {
        self.default_update_swimming();
    }

    /// Shared body of vanilla `Entity.updateSwimming` for player overrides.
    fn default_update_swimming(&self) {
        let Some(world) = self.level() else {
            return;
        };

        let block_fluid = get_fluid_state(&world, self.block_position());
        let swimming = select_swimming_state(
            self.is_swimming(),
            SwimmingEnvironment {
                sprinting: self
                    .as_living_entity()
                    .is_some_and(LivingEntity::is_sprinting),
                passenger: self.is_passenger(),
                in_water: self.is_in_water(),
                under_water: self.is_under_water(),
                block_fluid_is_water: block_fluid.is_water(),
            },
        );
        self.set_shared_swimming(swimming);
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

    /// Rotates this entity to face a fixed position.
    fn look_at(&self, from_anchor: EntityAnchor, target: DVec3) {
        apply_entity_look_at(self.as_entity_event_source(), from_anchor, target);
    }

    /// Rotates this entity to follow an anchored point on another entity.
    fn look_at_entity(
        &self,
        from_anchor: EntityAnchor,
        target: &dyn Entity,
        target_anchor: EntityAnchor,
    ) {
        self.look_at(from_anchor, target_anchor.position(target));
    }

    /// Returns vanilla `Entity.getYHeadRot`.
    fn head_yaw(&self) -> f32 {
        self.as_living_entity()
            .map_or(0.0, LivingEntity::y_head_rot)
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

    /// Mirrors vanilla `Entity.isInWall`.
    fn is_in_wall(&self) -> bool {
        if self.no_physics() {
            return false;
        }

        let Some(world) = self.level() else {
            return false;
        };

        let position = self.position();
        let check_width = f64::from(self.base().dimensions().width * 0.8);
        let eye_box = entity_eye_suffocation_box(
            DVec3::new(position.x, self.get_eye_y(), position.z),
            check_width,
        );

        !block_effects::for_each_block_in_aabb(eye_box, |pos| {
            let state = world.get_block_state(pos);
            !block_state_suffocates_eye_box(state, world.as_ref(), pos, eye_box)
        })
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

    /// Returns true when vanilla hurt-marked velocity sync is pending.
    fn hurt_marked(&self) -> bool {
        self.base().hurt_marked()
    }

    /// Marks this entity as hurt for vanilla self-inclusive motion sync.
    fn mark_hurt(&self) {
        self.base().mark_hurt();
    }

    /// Clears the vanilla hurt-marked motion sync flag.
    fn clear_hurt_mark(&self) {
        self.base().clear_hurt_mark();
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
        let Some(world) = self.level() else {
            return;
        };

        if self.hurt(
            &world,
            &DamageSource::environment(&vanilla_damage_types::LAVA),
            4.0,
        ) && self.should_play_lava_hurt_sound()
        {
            let pitch = 2.0 + rand::random::<f32>() * 0.4;
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

    /// Returns vanilla `Entity.getPercentFrozen`.
    fn percent_frozen(&self) -> f32 {
        let ticks_required = self.ticks_required_to_freeze();
        self.ticks_frozen().min(ticks_required) as f32 / ticks_required as f32
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

    /// Projects all base-owned synchronized fields into generated entity data.
    fn sync_base_entity_data(&self) {
        let Some(synced_data) = self.synced_data() else {
            return;
        };

        let save_data = self.base().save_data();
        synced_data.set_air_supply(save_data.air_supply);
        synced_data.set_custom_name(save_data.custom_name);
        synced_data.set_custom_name_visible(save_data.custom_name_visible);
        synced_data.set_silent(save_data.silent);
        synced_data.set_no_gravity(save_data.no_gravity);
        synced_data.set_base_glowing_flag(save_data.glowing);
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

    /// Propagates vanilla fall-damage handling to passengers.
    fn propagate_fall_to_passengers(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) {
        for passenger in self.passengers() {
            passenger.cause_fall_damage(fall_distance, damage_modifier, source);
        }
    }

    /// Applies vanilla fall damage.
    ///
    /// Living entities use the shared `LivingEntity` damage path; base entities
    /// only propagate to passengers and return `false`.
    fn cause_fall_damage(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool {
        if let Some(living) = self.as_living_entity() {
            return living.cause_living_fall_damage(fall_distance, damage_modifier, source);
        }
        if self.is_fall_damage_immune() {
            return false;
        }

        self.propagate_fall_to_passengers(fall_distance, damage_modifier, source);
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

        let speed = f64::from(rand::random::<f32>().mul_add(0.2, 0.1));
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

    /// Returns vanilla dimensions for a physical pose.
    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let dimensions = self.entity_type().dimensions;
        let Some(living) = self.as_living_entity() else {
            return dimensions;
        };

        if self.entity_type().fixed {
            dimensions
        } else {
            dimensions.scale(living.get_age_scale() * living.get_scale())
        }
    }

    /// Refreshes dimensions for the current physical pose.
    fn refresh_dimensions(&self) {
        let pose = self.pose();
        self.base()
            .set_pose_and_dimensions(pose, self.dimensions_for_pose(pose));
        // TODO: Fudge position after growth once free-position probing exists.
    }

    /// Sets the physical pose and synchronized pose metadata.
    fn set_pose(&self, pose: EntityPose) {
        self.base()
            .set_pose_and_dimensions(pose, self.dimensions_for_pose(pose));
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_pose(pose);
        }
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

    /// Returns the synchronized vanilla `Air` value.
    fn air_supply(&self) -> i32 {
        self.base().air_supply()
    }

    /// Sets the synchronized vanilla `Air` value.
    fn set_air_supply(&self, air_supply: i32) {
        self.base().set_air_supply(air_supply);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_air_supply(air_supply);
        }
    }

    /// Returns this entity's maximum vanilla air supply.
    fn max_air_supply(&self) -> i32 {
        DEFAULT_MAX_AIR_SUPPLY
    }

    /// Returns the vanilla portal cooldown in ticks.
    fn portal_cooldown(&self) -> i32 {
        self.base().portal_cooldown()
    }

    /// Sets the vanilla portal cooldown in ticks.
    fn set_portal_cooldown(&self, portal_cooldown: i32) {
        self.base().set_portal_cooldown(portal_cooldown);
    }

    /// Returns whether the entity is on vanilla portal cooldown.
    fn is_on_portal_cooldown(&self) -> bool {
        self.base().is_on_portal_cooldown()
    }

    /// Resets vanilla portal cooldown to this entity's dimension-changing delay.
    ///
    /// Mirrors vanilla `Entity.setPortalCooldown()`.
    fn reset_portal_cooldown(&self) {
        self.set_portal_cooldown(self.dimension_changing_delay());
    }

    /// Marks this entity as inside a vanilla portal during the current tick.
    ///
    /// Mirrors vanilla `Entity.setAsInsidePortal`.
    fn set_as_inside_portal(&self, portal: PortalKind, entry_position: BlockPos) {
        if self.is_on_portal_cooldown() {
            self.reset_portal_cooldown();
            return;
        }

        self.base().set_as_inside_portal(portal, entry_position);
    }

    /// Returns this entity's optional vanilla custom name.
    fn custom_name(&self) -> Option<TextComponent> {
        self.base().custom_name()
    }

    /// Sets this entity's optional vanilla custom name.
    fn set_custom_name(&self, custom_name: Option<TextComponent>) {
        self.base().set_custom_name(custom_name.clone());
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_custom_name(custom_name);
        }
    }

    /// Returns whether vanilla renders this entity's custom name.
    fn is_custom_name_visible(&self) -> bool {
        self.base().custom_name_visible()
    }

    /// Sets whether vanilla renders this entity's custom name.
    fn set_custom_name_visible(&self, visible: bool) {
        self.base().set_custom_name_visible(visible);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_custom_name_visible(visible);
        }
    }

    /// Returns whether this entity has the server-owned vanilla glowing tag.
    fn has_glowing_tag(&self) -> bool {
        self.base().glowing()
    }

    /// Sets this entity's server-owned vanilla glowing tag.
    fn set_glowing_tag(&self, glowing: bool) {
        self.base().set_glowing(glowing);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_base_glowing_flag(glowing);
        }
    }

    /// Returns a snapshot of this entity's vanilla scoreboard tags.
    fn tags(&self) -> Vec<String> {
        self.base().tags()
    }

    /// Adds a vanilla scoreboard tag.
    fn add_tag(&self, tag: String) -> bool {
        self.base().add_tag(tag)
    }

    /// Removes a vanilla scoreboard tag.
    fn remove_tag(&self, tag: &str) -> bool {
        self.base().remove_tag(tag)
    }

    /// Returns a snapshot of this entity's vanilla custom data.
    fn custom_data(&self) -> NbtCompound {
        self.base().custom_data()
    }

    /// Replaces this entity's vanilla custom data.
    fn set_custom_data(&self, custom_data: NbtCompound) {
        self.base().set_custom_data(custom_data);
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
        self.base().silent()
    }

    /// Sets whether sounds from this entity are suppressed.
    fn set_silent(&self, silent: bool) {
        self.base().set_silent(silent);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_silent(silent);
        }
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
        let pitch = 1.6 + (rand::random::<f32>() - rand::random::<f32>()) * 0.4;
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
        let pitch = 1.0 + (rand::random::<f32>() - rand::random::<f32>()) * 0.4;
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
        self.base().no_gravity()
    }

    /// Sets the shared vanilla `NoGravity` flag.
    fn set_no_gravity(&self, no_gravity: bool) {
        self.base().set_no_gravity(no_gravity);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_no_gravity(no_gravity);
        }
    }

    /// Returns the shared vanilla `Invulnerable` flag.
    fn is_invulnerable(&self) -> bool {
        self.base().invulnerable()
    }

    /// Sets the shared vanilla `Invulnerable` flag.
    fn set_invulnerable(&self, invulnerable: bool) {
        self.base().set_invulnerable(invulnerable);
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
            let previous_test_area = test_area.translate(DVec3::new(-movement.x, 0.0, -movement.z));
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

    /// Spawns an item at this entity's location plus a vanilla attachment offset.
    fn spawn_at_location_with_offset(
        &self,
        item: ItemStack,
        offset: DVec3,
    ) -> Option<Arc<entities::ItemEntity>> {
        let world = self.level()?;
        world.spawn_item(self.position() + offset, item)
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
    fn load_additional(&self, _nbt: BorrowedNbtCompoundView<'_, '_>) {}

    /// Applies damage to this entity.
    ///
    /// Vanilla: `Entity.hurtServer()` — overridden by `LivingEntity` (complex
    /// armor/effects/invulnerability logic) and `ItemEntity` (health decrement
    /// and discard). `world` must preserve vanilla's explicit `ServerLevel`
    /// argument rather than being inferred from the target. Default returns
    /// `false` (entity ignores damage).
    #[expect(
        unused_variables,
        reason = "default trait impl; parameters used by overrides"
    )]
    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        false
    }
}

pub(crate) fn apply_entity_look_at(entity: &dyn Entity, from_anchor: EntityAnchor, target: DVec3) {
    let rotation = look_at_rotation(from_anchor.position(entity), target);
    entity.set_rotation(rotation);
    let rotation = entity.rotation();
    if let Some(living) = entity.as_living_entity() {
        living.set_y_head_rot(rotation.0);
    }
    entity.base().set_old_rotation_to_current();
}

fn look_at_rotation(from: DVec3, target: DVec3) -> (f32, f32) {
    let delta = target - from;
    let horizontal = delta.x.hypot(delta.z);
    let pitch = wrap_look_at_degrees(-delta.y.atan2(horizontal).to_degrees() as f32);
    let yaw = wrap_look_at_degrees(delta.z.atan2(delta.x).to_degrees() as f32 - 90.0);
    (yaw, pitch)
}

fn wrap_look_at_degrees(mut degrees: f32) -> f32 {
    degrees %= 360.0;
    if degrees >= 180.0 {
        degrees -= 360.0;
    }
    if degrees < -180.0 {
        degrees += 360.0;
    }
    degrees
}

#[cfg(test)]
mod look_at_tests {
    use glam::DVec3;

    use super::look_at_rotation;

    #[test]
    fn look_at_rotation_matches_vanilla_axes() {
        assert_eq!(
            look_at_rotation(DVec3::ZERO, DVec3::new(0.0, 0.0, 1.0)),
            (0.0, 0.0)
        );
        assert_eq!(
            look_at_rotation(DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0)),
            (-90.0, 0.0)
        );
        assert_eq!(
            look_at_rotation(DVec3::ZERO, DVec3::new(0.0, 1.0, 1.0)),
            (0.0, -45.0)
        );
        assert_eq!(
            look_at_rotation(DVec3::ZERO, DVec3::new(-1.0, 0.0, -1.0)),
            (135.0, 0.0)
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

    /// Returns vanilla living body/head rotation state.
    fn living_rotation_state(&self) -> LivingRotationState {
        self.living_base().rotation_state()
    }

    /// Returns vanilla `LivingEntity.yBodyRot`.
    fn y_body_rot(&self) -> f32 {
        self.living_base().y_body_rot()
    }

    /// Sets vanilla `LivingEntity.yBodyRot`.
    fn set_y_body_rot(&self, y_body_rot: f32) {
        self.living_base().set_y_body_rot(y_body_rot);
    }

    /// Returns vanilla `LivingEntity.yHeadRot`.
    fn y_head_rot(&self) -> f32 {
        self.living_base().y_head_rot()
    }

    /// Sets vanilla `LivingEntity.yHeadRot`.
    fn set_y_head_rot(&self, y_head_rot: f32) {
        self.living_base().set_y_head_rot(y_head_rot);
    }

    /// Copies current living body/head rotations to vanilla old-rotation state.
    fn advance_living_rotation_for_base_tick(&self) {
        self.living_base().advance_rotation_for_base_tick();
    }

    /// Copies current attack animation to vanilla old attack-animation state.
    fn advance_attack_animation_for_base_tick(&self) {
        self.living_base().advance_attack_animation_for_base_tick();
    }

    /// Runs vanilla `LivingEntity.baseTick`.
    fn base_tick_living_entity(&self) {
        self.advance_living_rotation_for_base_tick();
        self.advance_attack_animation_for_base_tick();
        self.entity_base_tick();
        self.tick_living_environmental_damage();
    }

    /// Returns vanilla arm-swing animation state.
    fn living_swing_state(&self) -> LivingSwingState {
        self.living_base().swing_state()
    }

    /// Returns vanilla `LivingEntity.getCurrentSwingDuration`.
    fn current_swing_duration(&self) -> i32 {
        // TODO: Use the held item's SWING_ANIMATION component once it has typed component data.
        let swing_duration = DEFAULT_SWING_DURATION;
        if let Some(haste) = self.mob_effect(vanilla_mob_effects::HASTE) {
            swing_duration - (1 + haste.amplifier())
        } else if let Some(mining_fatigue) = self.mob_effect(vanilla_mob_effects::MINING_FATIGUE) {
            swing_duration + (1 + mining_fatigue.amplifier()) * 2
        } else {
            swing_duration
        }
    }

    /// Runs vanilla `LivingEntity.swing`.
    fn swing(&self, hand: InteractionHand, update_self: bool) {
        if !self
            .living_base()
            .start_swing(hand, self.current_swing_duration())
        {
            return;
        }

        let Some(world) = self.level() else {
            return;
        };
        let action = match hand {
            InteractionHand::MainHand => AnimateAction::SwingMainHand,
            InteractionHand::OffHand => AnimateAction::SwingOffHand,
        };
        let packet = CAnimate::new(self.id(), action);
        let exclude = if update_self { None } else { Some(self.id()) };
        world.broadcast_to_entity_trackers(self.id(), packet.clone(), exclude);
        if update_self && let Some(player) = self.as_player() {
            player.send_packet(packet);
        }
    }

    /// Runs vanilla `LivingEntity.updateSwingTime`.
    fn update_swing_time(&self) {
        self.living_base()
            .update_swing_time(self.current_swing_duration());
    }

    /// Returns a reference to this entity's attribute map.
    fn attributes(&self) -> &SyncMutex<AttributeMap> {
        self.living_base().attributes()
    }

    /// Appends vanilla-shaped living state used by command NBT predicates.
    fn save_command_nbt(&self, nbt: &mut NbtCompound) {
        nbt.insert("Health", self.get_health());
        nbt.insert(
            "DeathTime",
            NbtTag::Short(self.living_base().death_time() as i16),
        );
        nbt.insert("AbsorptionAmount", self.get_absorption_amount());
        nbt.insert(
            "current_impulse_context_reset_grace_time",
            self.living_base()
                .current_impulse_context_reset_grace_time(),
        );
        if let Some(impact) = self.living_base().current_impulse_impact_pos() {
            nbt.insert(
                "current_explosion_impact_pos",
                NbtList::Double(vec![impact.x, impact.y, impact.z]),
            );
        }
        nbt.insert("attributes", self.attributes().lock().to_vanilla_nbt());

        let mut effects = self.living_base().active_mob_effects();
        effects.sort_by_key(|effect| effect.effect().try_id().unwrap_or(usize::MAX));
        if !effects.is_empty() {
            nbt.insert(
                "active_effects",
                NbtList::Compound(
                    effects
                        .iter()
                        .map(ActiveMobEffect::to_vanilla_nbt)
                        .collect(),
                ),
            );
        }

        nbt.insert("FallFlying", nbt_bool(self.is_fall_flying()));
        if let Some(pos) = self.sleeping_pos() {
            nbt.insert(
                "sleeping_pos",
                NbtTag::IntArray(vec![pos.x(), pos.y(), pos.z()]),
            );
        }
        if let Some(uuid) = self.last_hurt_by_player_uuid() {
            nbt.insert(
                "last_hurt_by_player",
                NbtTag::IntArray(uuid.to_int_array().to_vec()),
            );
            nbt.insert(
                "last_hurt_by_player_memory_time",
                self.last_hurt_by_player_memory_time(),
            );
        }
        if let Some(entity) = self.last_hurt_by_mob() {
            nbt.insert(
                "last_hurt_by_mob",
                NbtTag::IntArray(entity.uuid().to_int_array().to_vec()),
            );
            nbt.insert(
                "ticks_since_last_hurt_by_mob",
                self.tick_count()
                    .wrapping_sub(self.last_hurt_by_mob_timestamp()),
            );
        }

        let mut equipment = NbtCompound::new();
        for slot in EquipmentSlot::ALL {
            self.with_equipment_slot(slot, &mut |item| {
                if !item.is_empty() {
                    equipment.insert(slot.name(), item.to_nbt_tag_ref());
                }
            });
        }
        if !equipment.is_empty() {
            nbt.insert("equipment", NbtTag::Compound(equipment));
        }
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

    /// Returns vanilla `LivingEntity.noActionTime`.
    fn no_action_time(&self) -> i32 {
        self.living_base().no_action_time()
    }

    /// Sets vanilla `LivingEntity.noActionTime`.
    fn set_no_action_time(&self, no_action_time: i32) {
        self.living_base().set_no_action_time(no_action_time);
    }

    /// Increments vanilla `LivingEntity.noActionTime`.
    fn increment_no_action_time(&self) {
        self.living_base().increment_no_action_time();
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

    /// Returns vanilla `LivingEntity.isBaby()`.
    fn is_baby(&self) -> bool {
        false
    }

    /// Returns vanilla `LivingEntity.getSoundVolume`.
    fn sound_volume(&self) -> f32 {
        1.0
    }

    /// Returns vanilla `LivingEntity.getVoicePitch`.
    fn voice_pitch(&self) -> f32 {
        if self.is_baby() {
            (rand::random::<f32>() - rand::random::<f32>()) * 0.2 + 1.5
        } else {
            (rand::random::<f32>() - rand::random::<f32>()) * 0.2 + 1.0
        }
    }

    /// Returns vanilla `LivingEntity.getHurtSound`.
    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_GENERIC_HURT)
    }

    /// Returns vanilla `LivingEntity.getDeathSound`.
    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_GENERIC_DEATH)
    }

    /// Runs vanilla `LivingEntity.makeSound`.
    fn make_sound(&self, sound: Option<SoundEventRef>) {
        if let Some(sound) = sound {
            self.play_sound(sound, self.sound_volume(), self.voice_pitch());
        }
    }

    /// Runs vanilla `LivingEntity.playHurtSound`.
    fn play_hurt_sound(&self, source: &DamageSource) {
        if let Some(mob) = self.as_mob() {
            mob.reset_ambient_sound_time();
        }
        self.make_sound(self.hurt_sound(source));
    }

    /// Plays vanilla's death sound for this living entity.
    fn play_death_sound(&self) {
        self.make_sound(self.death_sound());
    }

    /// Returns vanilla `LivingEntity.getAgeScale()`.
    fn get_age_scale(&self) -> f32 {
        if self.is_baby() { 0.5 } else { 1.0 }
    }

    /// Returns vanilla `LivingEntity.getScale()`.
    fn get_scale(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::SCALE)
            .unwrap_or(1.0) as f32
    }

    /// Returns true if the entity is alive (health > 0).
    fn is_alive(&self) -> bool {
        !self.is_dead_or_dying()
    }

    /// Returns vanilla `LivingEntity.getArmorCoverPercentage()`.
    fn get_armor_cover_percentage(&self) -> f32 {
        let mut covered_slots = 0;
        for slot in EquipmentSlot::ARMOR_SLOTS {
            self.with_equipment_slot(slot, &mut |item_stack| {
                if !item_stack.is_empty() {
                    covered_slots += 1;
                }
            });
        }

        covered_slots as f32 / EquipmentSlot::ARMOR_SLOTS.len() as f32
    }

    /// Returns vanilla `LivingEntity.getVisibilityPercent()`.
    fn get_visibility_percent(&self, targeting_entity: Option<&dyn Entity>) -> f64 {
        let mut visibility_percent = 1.0;
        if self.is_discrete() {
            visibility_percent *= 0.8;
        }

        if self.is_invisible() {
            visibility_percent *= 0.7 * f64::from(self.get_armor_cover_percentage().max(0.1));
        }

        if self.disguise_head_matches_targeting_entity(targeting_entity) {
            visibility_percent *= 0.5;
        }

        visibility_percent
    }

    /// Returns whether the equipped head item reduces visibility to `targeting_entity`.
    fn disguise_head_matches_targeting_entity(
        &self,
        targeting_entity: Option<&dyn Entity>,
    ) -> bool {
        let Some(targeting_entity) = targeting_entity else {
            return false;
        };

        let mut matches_target = false;
        self.with_equipment_slot(EquipmentSlot::Head, &mut |item_stack| {
            let target_type = targeting_entity.entity_type();
            matches_target = target_type == &vanilla_entities::SKELETON
                && item_stack.is(&vanilla_items::ITEMS.skeleton_skull)
                || target_type == &vanilla_entities::ZOMBIE
                    && item_stack.is(&vanilla_items::ITEMS.zombie_head)
                || target_type == &vanilla_entities::PIGLIN
                    && item_stack.is(&vanilla_items::ITEMS.piglin_head)
                || target_type == &vanilla_entities::PIGLIN_BRUTE
                    && item_stack.is(&vanilla_items::ITEMS.piglin_head)
                || target_type == &vanilla_entities::CREEPER
                    && item_stack.is(&vanilla_items::ITEMS.creeper_head);
        });
        matches_target
    }

    /// Returns vanilla `LivingEntity.canBeSeenByAnyone()`.
    fn can_be_seen_by_anyone(&self) -> bool {
        !self.is_spectator() && Entity::is_alive(self)
    }

    /// Returns vanilla `LivingEntity.canBeSeenAsEnemy()`.
    fn can_be_seen_as_enemy(&self) -> bool {
        !self.is_invulnerable() && self.can_be_seen_by_anyone()
    }

    /// Returns vanilla `LivingEntity.canAttack()`.
    fn can_attack(&self, target: &dyn LivingEntity) -> bool {
        if target.entity_type() == &vanilla_entities::PLAYER
            && self
                .level()
                .is_some_and(|world| world.difficulty() == Difficulty::Peaceful)
        {
            return false;
        }

        target.can_be_seen_as_enemy()
    }

    /// Returns vanilla `LivingEntity.getLastDamageSource()`.
    fn last_damage_source(&self) -> Option<DamageSource> {
        let game_time = self.level().map_or(0, |world| world.game_time());
        self.living_base().last_damage_source(game_time)
    }

    /// Sets vanilla `LivingEntity.lastHurtByPlayer`.
    fn set_last_hurt_by_player(&self, player_uuid: Uuid, time_to_remember: i32) {
        self.living_base()
            .set_last_hurt_by_player(player_uuid, time_to_remember);
    }

    /// Returns vanilla `LivingEntity.lastHurtByPlayerMemoryTime`.
    fn last_hurt_by_player_memory_time(&self) -> i32 {
        self.living_base().last_hurt_by_player_memory_time()
    }

    /// Returns vanilla `LivingEntity.lastHurtByPlayer`, if still remembered.
    fn last_hurt_by_player_uuid(&self) -> Option<Uuid> {
        self.living_base().last_hurt_by_player_uuid()
    }

    /// Returns vanilla `LivingEntity.lastHurtByMob`.
    fn last_hurt_by_mob(&self) -> Option<SharedEntity> {
        self.living_base().last_hurt_by_mob()
    }

    /// Returns vanilla `LivingEntity.lastHurtByMobTimestamp`.
    fn last_hurt_by_mob_timestamp(&self) -> i32 {
        self.living_base().last_hurt_by_mob_timestamp()
    }

    /// Sets vanilla `LivingEntity.lastHurtByMob`.
    fn set_last_hurt_by_mob(&self, target: Option<&SharedEntity>) {
        self.living_base()
            .set_last_hurt_by_mob(target, self.tick_count());
    }

    /// Returns vanilla `LivingEntity.lastHurtMob`.
    fn last_hurt_mob(&self) -> Option<SharedEntity> {
        self.living_base().last_hurt_mob()
    }

    /// Returns vanilla `LivingEntity.lastHurtMobTimestamp`.
    fn last_hurt_mob_timestamp(&self) -> i32 {
        self.living_base().last_hurt_mob_timestamp()
    }

    /// Sets vanilla `LivingEntity.lastHurtMob`.
    fn set_last_hurt_mob(&self, target: Option<&SharedEntity>) {
        self.living_base()
            .set_last_hurt_mob(target, self.tick_count());
    }

    /// Resolves vanilla `LivingEntity.resolveMobResponsibleForDamage`.
    fn resolve_mob_responsible_for_damage(&self, world: &World, source: &DamageSource) {
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::NO_ANGER) {
            return;
        }
        if source.damage_type == &vanilla_damage_types::WIND_CHARGE
            && REGISTRY.entity_types.is_in_tag(
                self.entity_type(),
                &EntityTypeTag::NO_ANGER_FROM_WIND_CHARGE,
            )
        {
            return;
        }

        let Some(entity_id) = source.causing_entity_id else {
            return;
        };
        let Some(entity) = world.get_entity_by_id(entity_id) else {
            return;
        };
        if entity.is_living_entity() {
            self.set_last_hurt_by_mob(Some(&entity));
        }
    }

    /// Resolves vanilla `LivingEntity.resolvePlayerResponsibleForDamage`.
    fn resolve_player_responsible_for_damage(&self, world: &World, source: &DamageSource) {
        let Some(entity_id) = source.causing_entity_id else {
            return;
        };
        let Some(entity) = world.get_entity_by_id(entity_id) else {
            return;
        };
        if entity.entity_type() == &vanilla_entities::PLAYER {
            self.set_last_hurt_by_player(entity.uuid(), 100);
        }
    }

    /// Returns vanilla `LivingEntity.hasLineOfSight()`.
    fn has_line_of_sight(&self, target: &dyn Entity) -> bool {
        self.has_line_of_sight_with(
            target,
            ClipBlockShape::Collider,
            ClipFluid::None,
            target.get_eye_y(),
        )
    }

    /// Returns vanilla line-of-sight with explicit clip options.
    fn has_line_of_sight_with(
        &self,
        target: &dyn Entity,
        block_shape: ClipBlockShape,
        fluid: ClipFluid,
        target_eye_y: f64,
    ) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let Some(target_world) = target.level() else {
            return false;
        };
        if !Arc::ptr_eq(&world, &target_world) {
            return false;
        }

        let position = self.position();
        let target_position = target.position();
        let start = DVec3::new(position.x, self.get_eye_y(), position.z);
        let end = DVec3::new(target_position.x, target_eye_y, target_position.z);
        if start.distance_squared(end) > 128.0 * 128.0 {
            return false;
        }

        world.clip(start, end, block_shape, fluid).is_miss()
    }

    /// Returns vanilla base living-entity invulnerability.
    fn default_is_invulnerable_to(&self, source: &DamageSource) -> bool {
        self.is_removed()
            || self.is_invulnerable() && !source.bypasses_invulnerability()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FIRE) && self.fire_immune()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FALL)
                && self.is_fall_damage_immune()
    }

    /// Returns whether this living entity ignores a damage source.
    fn is_invulnerable_to(&self, world: &World, source: &DamageSource) -> bool {
        self.default_is_invulnerable_to(source)
            || enchantment_helper::is_immune_to_damage(world, self, source)
    }

    /// Main vanilla living-entity damage entry point.
    ///
    /// `world` is the `ServerLevel` supplied by the vanilla caller. It may
    /// intentionally differ from the entity's attached world.
    fn hurt_server(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        if self.is_invulnerable_to(world, source) {
            return false;
        }
        if self.is_dead_or_dying() {
            return false;
        }
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FIRE)
            && self.has_mob_effect(vanilla_mob_effects::FIRE_RESISTANCE)
        {
            return false;
        }
        if self.is_sleeping() {
            self.stop_sleeping();
        }

        self.set_no_action_time(0);

        let mut damage = amount;
        if damage < 0.0 {
            damage = 0.0;
        }

        // TODO: apply item blocking before actually_hurt once shield/use-item hooks exist.
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FREEZING)
            && REGISTRY
                .entity_types
                .is_in_tag(self.entity_type(), &EntityTypeTag::FREEZE_HURTS_EXTRA_TYPES)
        {
            damage *= 5.0;
        }
        // TODO: apply helmet damage once those equipment hooks exist.
        if !damage.is_finite() {
            damage = f32::MAX;
        }

        let Some((took_full_damage, effective_amount)) = self
            .living_base()
            .apply_damage_cooldown(damage, source.bypasses_cooldown())
        else {
            return false;
        };

        self.before_actually_hurt(source, effective_amount);
        self.actually_hurt(world, source, effective_amount);
        self.resolve_mob_responsible_for_damage(world, source);
        self.resolve_player_responsible_for_damage(world, source);

        if took_full_damage {
            self.broadcast_damage_event(world, source);
            if !source.is(&vanilla_damage_type_tags::DamageTypeTag::NO_IMPACT) {
                self.mark_hurt();
                self.broadcast_hurt_animation(world);
            }
            self.apply_damage_knockback(source);
        }

        if self.is_dead_or_dying() {
            if took_full_damage {
                self.play_death_sound();
            }
            self.die(source);
        } else if took_full_damage {
            self.play_hurt_sound(source);
        }
        // TODO: Play secondary hurt sounds once equipment effects expose them.

        let game_time = self.level().map_or(0, |world| world.game_time());
        self.living_base()
            .record_last_damage_source(source, game_time);

        true
    }

    /// Hook before applying damage after vanilla reductions.
    fn before_actually_hurt(&self, _source: &DamageSource, _amount: f32) {}

    /// Applies damage after vanilla reductions.
    fn actually_hurt(&self, _world: &World, _source: &DamageSource, amount: f32) {
        if amount <= 0.0 {
            return;
        }

        self.set_health(self.get_health() - amount);
    }

    /// Applies vanilla hurt knockback for a damage source.
    fn apply_damage_knockback(&self, source: &DamageSource) {
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::NO_KNOCKBACK) {
            return;
        }

        let (xd, zd) = self.damage_knockback_direction(source);
        self.knockback(DAMAGE_KNOCKBACK_POWER, xd, zd);
        self.indicate_damage(xd, zd);
    }

    /// Returns the horizontal direction used by vanilla damage knockback.
    fn damage_knockback_direction(&self, source: &DamageSource) -> (f64, f64) {
        // TODO: when projectile entities expose calculateHorizontalHurtKnockbackDirection,
        // use the direct entity hook before falling back to source_position.
        let Some(source_position) = source.source_position else {
            return (0.0, 0.0);
        };

        let position = self.position();
        (
            source_position.x - position.x,
            source_position.z - position.z,
        )
    }

    /// Applies vanilla `LivingEntity.knockback`.
    fn knockback(&self, mut power: f64, mut xd: f64, mut zd: f64) {
        power *= 1.0 - self.knockback_resistance();
        if power <= 0.0 {
            return;
        }

        while xd * xd + zd * zd < KNOCKBACK_DIRECTION_EPSILON_SQ {
            xd = (rand::random::<f64>() - rand::random::<f64>()) * 0.01;
            zd = (rand::random::<f64>() - rand::random::<f64>()) * 0.01;
        }

        let old_velocity = self.velocity();
        let delta_vector = DVec3::new(xd, 0.0, zd).normalize() * power;
        self.set_velocity(DVec3::new(
            old_velocity.x / 2.0 - delta_vector.x,
            if self.on_ground() {
                0.4_f64.min(old_velocity.y / 2.0 + power)
            } else {
                old_velocity.y
            },
            old_velocity.z / 2.0 - delta_vector.z,
        ));
        self.mark_velocity_sync();
    }

    /// Returns vanilla knockback resistance.
    fn knockback_resistance(&self) -> f64 {
        self.attributes()
            .lock()
            .required_value(vanilla_attributes::KNOCKBACK_RESISTANCE)
    }

    /// Mirrors vanilla `LivingEntity.indicateDamage`.
    fn indicate_damage(&self, _xd: f64, _zd: f64) {}

    /// Returns the chunk used for vanilla nearby hurt broadcasts.
    fn hurt_broadcast_chunk(&self) -> ChunkPos {
        ChunkPos::from_entity_pos(self.position())
    }

    /// Broadcasts vanilla damage-event metadata near this entity.
    fn broadcast_damage_event(&self, world: &World, source: &DamageSource) {
        world.broadcast_to_nearby(
            self.hurt_broadcast_chunk(),
            CDamageEvent {
                entity_id: self.id(),
                source_type_id: source.damage_type.id() as i32,
                source_cause_id: source.causing_entity_id.map_or(0, |id| id + 1),
                source_direct_id: source.direct_entity_id.map_or(0, |id| id + 1),
                source_position: source.source_position,
            },
            None,
        );
    }

    /// Broadcasts vanilla hurt animation near this entity.
    fn broadcast_hurt_animation(&self, world: &World) {
        let (yaw, _) = self.rotation();
        world.broadcast_to_nearby(
            self.hurt_broadcast_chunk(),
            CHurtAnimation {
                entity_id: self.id(),
                yaw,
            },
            None,
        );
    }

    /// Processes vanilla living death side effects.
    fn die(&self, source: &DamageSource) {
        if self.is_removed() {
            return;
        }
        if !self.living_base().mark_death_processed() {
            return;
        }

        self.game_event(&vanilla_game_events::ENTITY_DIE);
        self.drop_all_death_loot(source);
        self.broadcast_entity_event(EntityStatus::Death);
        self.set_pose(EntityPose::Dying);
    }

    /// Returns vanilla `LivingEntity.shouldDropLoot`.
    fn should_drop_loot(&self, world: &World) -> bool {
        !self.is_baby() && world.get_game_rule(&MOB_DROPS).as_bool() == Some(true)
    }

    /// Returns vanilla `LivingEntity.shouldDropExperience`.
    fn should_drop_experience(&self) -> bool {
        !self.is_baby()
    }

    /// Returns vanilla `LivingEntity.isAlwaysExperienceDropper`.
    fn is_always_experience_dropper(&self) -> bool {
        false
    }

    /// Runs vanilla `LivingEntity.skipDropExperience`.
    fn skip_drop_experience(&self) {
        self.living_base().skip_drop_experience();
    }

    /// Returns vanilla `LivingEntity.wasExperienceConsumed`.
    fn was_experience_consumed(&self) -> bool {
        self.living_base().was_experience_consumed()
    }

    /// Returns vanilla `LivingEntity.getBaseExperienceReward`.
    fn base_experience_reward(&self) -> i32 {
        if let Some(animal) = self.as_animal() {
            return animal.base_experience_reward_animal();
        }

        self.as_mob().map_or(0, Mob::base_experience_reward_mob)
    }

    /// Returns vanilla `LivingEntity.getExperienceReward`.
    fn experience_reward(&self, _world: &World, _killer_entity_id: Option<i32>) -> i32 {
        // TODO: Apply EnchantmentHelper.processMobExperience once enchantment
        // value-effect hooks can receive the killer/living-entity context.
        self.base_experience_reward()
    }

    /// Runs the currently implemented subset of vanilla `LivingEntity.dropAllDeathLoot`.
    fn drop_all_death_loot(&self, source: &DamageSource) {
        let Some(world) = self.level() else {
            return;
        };
        if self.should_drop_loot(world.as_ref()) {
            let killed_by_player = self.last_hurt_by_player_memory_time() > 0;
            self.drop_from_loot_table(source, killed_by_player);
            self.drop_custom_death_loot(source, killed_by_player);
            if let Some(mob) = self.as_mob() {
                mob.drop_custom_death_loot_mob(source, killed_by_player);
            }
        }
        self.drop_experience(&world, source.causing_entity_id);
        // TODO: Drop non-mob equipment overrides once those foundations exist.
    }

    /// Runs vanilla `LivingEntity.dropExperience`.
    fn drop_experience(&self, world: &Arc<World>, killer_entity_id: Option<i32>) {
        if self.was_experience_consumed() {
            return;
        }

        let should_drop = self.is_always_experience_dropper()
            || self.last_hurt_by_player_memory_time() > 0
                && self.should_drop_experience()
                && world.get_game_rule(&MOB_DROPS).as_bool() == Some(true);
        if !should_drop {
            return;
        }

        let reward = self.experience_reward(world, killer_entity_id);
        if reward > 0 {
            ExperienceOrbEntity::award(world, self.position(), reward);
        }
    }

    /// Resolves the loot table used by vanilla `LivingEntity.dropFromLootTable`.
    fn death_loot_table(&self) -> Option<LootTableRef> {
        if let Some(mob) = self.as_mob()
            && mob.has_custom_death_loot_table()
        {
            return mob.custom_death_loot_table();
        }

        let entity_type = self.entity_type();
        let loot_key = Identifier::vanilla(format!("entities/{}", entity_type.key.path));
        REGISTRY.loot_tables.by_key(&loot_key)
    }

    /// Returns vanilla `Entity.getLootTableSeed` for death loot.
    fn death_loot_table_seed(&self) -> i64 {
        self.as_mob().map_or(0, Mob::death_loot_table_seed)
    }

    /// Runs vanilla `LivingEntity.dropFromLootTable`.
    fn drop_from_loot_table(&self, source: &DamageSource, killed_by_player: bool) {
        let Some(world) = self.level() else {
            return;
        };
        let has_custom_death_loot_table =
            self.as_mob().is_some_and(Mob::has_custom_death_loot_table);
        let Some(loot_table) = self.death_loot_table() else {
            if has_custom_death_loot_table && let Some(mob) = self.as_mob() {
                mob.clear_custom_death_loot_table();
            }
            return;
        };

        let seed = self.death_loot_table_seed();
        let drops = if seed == 0 {
            let mut rng = rand::rng();
            death_loot_items_with_rng(
                self,
                loot_table,
                world.as_ref(),
                source,
                killed_by_player,
                &mut rng,
            )
        } else {
            let mut rng = StdRng::seed_from_u64(seed as u64);
            death_loot_items_with_rng(
                self,
                loot_table,
                world.as_ref(),
                source,
                killed_by_player,
                &mut rng,
            )
        };

        if has_custom_death_loot_table && let Some(mob) = self.as_mob() {
            mob.clear_custom_death_loot_table();
        }

        for item_stack in drops {
            self.spawn_at_location(item_stack, 0.0);
        }
    }

    /// Hook for non-mob custom death loot.
    fn drop_custom_death_loot(&self, _source: &DamageSource, _killed_by_player: bool) {}

    /// Ticks the vanilla living death animation and removes the entity at completion.
    fn tick_death(&self) {
        let death_time = self.living_base().increment_death_time();
        if death_time >= DEATH_DURATION && !self.is_removed() {
            self.broadcast_entity_event(EntityStatus::Poof);
            self.set_removed(RemovalReason::Killed);
        }
    }

    /// Gets the absorption amount (extra health from effects like absorption).
    fn get_absorption_amount(&self) -> f32 {
        self.living_base().absorption_amount()
    }

    /// Sets the absorption amount.
    fn set_absorption_amount(&self, amount: f32) {
        self.living_base().set_absorption_amount(amount);
    }

    /// Returns vanilla `LivingEntity.getFallDamageSound()`.
    fn fall_damage_sound(&self, damage: i32) -> SoundEventRef {
        let (small, big) = self.fall_sounds();
        if damage > 4 { big } else { small }
    }

    /// Plays vanilla `LivingEntity.playBlockFallSound()`.
    fn play_block_fall_sound(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let position = self.position();
        let pos = BlockPos::new(
            position.x.floor() as i32,
            (position.y - f64::from(0.2_f32)).floor() as i32,
            position.z.floor() as i32,
        );
        let state = world.get_block_state(pos);
        if state.is_air() {
            return;
        }

        let sound_type = state.get_block().config.sound_type;
        self.play_sound(
            sound_type.fall_sound,
            sound_type.volume * 0.5,
            sound_type.pitch * 0.75,
        );
    }

    /// Mirrors vanilla `LivingEntity.causeFallDamage`.
    fn cause_living_fall_damage(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool {
        let effective_fall_distance =
            if let Some(impact_pos) = self.living_base().current_impulse_impact_pos() {
                let effective_fall_distance = fall_distance.min(impact_pos.y - self.position().y);
                if effective_fall_distance <= 0.0 {
                    self.reset_current_impulse_context();
                } else {
                    self.try_reset_current_impulse_context();
                }
                effective_fall_distance
            } else {
                fall_distance
            };

        if self.is_fall_damage_immune() {
            return false;
        }

        self.propagate_fall_to_passengers(effective_fall_distance, damage_modifier, source);

        let attributes = self.attributes().lock();
        let safe_fall_distance = attributes
            .get_value(vanilla_attributes::SAFE_FALL_DISTANCE)
            .unwrap_or(vanilla_attributes::SAFE_FALL_DISTANCE.default_value);
        let fall_damage_multiplier = attributes
            .get_value(vanilla_attributes::FALL_DAMAGE_MULTIPLIER)
            .unwrap_or(vanilla_attributes::FALL_DAMAGE_MULTIPLIER.default_value);
        drop(attributes);

        let damage = LivingEntityBase::calculate_fall_damage(
            effective_fall_distance,
            damage_modifier,
            safe_fall_distance,
            fall_damage_multiplier,
        );
        if damage <= 0 {
            return false;
        }

        self.reset_current_impulse_context();
        self.play_sound(self.fall_damage_sound(damage), 1.0, 1.0);
        self.play_block_fall_sound();
        if let Some(world) = self.level() {
            self.hurt(&world, source, damage as f32);
        }
        true
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

    /// Returns all active vanilla mob effects.
    fn active_mob_effects(&self) -> Vec<ActiveMobEffect> {
        self.living_base().active_mob_effects()
    }

    /// Sets active vanilla mob-effect state.
    fn set_mob_effect(&self, effect: MobEffectRef, amplifier: i32) {
        self.add_mob_effect(MobEffectInstance::new(effect, amplifier));
    }

    /// Adds or updates active vanilla mob-effect state.
    fn add_mob_effect(&self, effect: MobEffectInstance) -> bool {
        if !self.is_affected_by_potions() {
            return false;
        }
        self.living_base().add_mob_effect(effect)
    }

    /// Sets the presence of a vanilla mob effect.
    fn set_mob_effect_active(&self, effect: MobEffectRef, active: bool) {
        if active {
            self.set_mob_effect(effect, 0);
        } else {
            self.remove_mob_effect(effect);
        }
    }

    /// Removes active vanilla mob-effect state.
    fn remove_mob_effect(&self, effect: MobEffectRef) -> bool {
        self.living_base().remove_mob_effect(effect)
    }

    /// Ticks vanilla mob-effect durations.
    fn tick_mob_effects(&self) {
        self.living_base().tick_mob_effects();
    }

    /// Returns whether vanilla effects keep this entity from drowning.
    fn has_water_breathing(&self) -> bool {
        self.has_mob_effect(vanilla_mob_effects::WATER_BREATHING)
            || self.has_mob_effect(vanilla_mob_effects::CONDUIT_POWER)
            || self.has_mob_effect(vanilla_mob_effects::BREATH_OF_THE_NAUTILUS)
    }

    /// Returns whether active vanilla effects refill this entity's air supply.
    fn should_effects_refill_air_supply(&self) -> bool {
        !self.has_mob_effect(vanilla_mob_effects::BREATH_OF_THE_NAUTILUS)
            || self.has_mob_effect(vanilla_mob_effects::WATER_BREATHING)
            || self.has_mob_effect(vanilla_mob_effects::CONDUIT_POWER)
    }

    /// Returns vanilla `LivingEntity.canBreatheUnderwater`.
    fn can_breathe_underwater(&self) -> bool {
        self.entity_type().flags.can_breathe_underwater
    }

    /// Returns whether this entity can lose air and take drowning damage.
    fn can_drown_in_water(&self) -> bool {
        if self.can_breathe_underwater() || self.has_water_breathing() {
            return false;
        }

        !self
            .as_player()
            .is_some_and(|player| player.abilities.lock().invulnerable)
    }

    /// Returns whether the entity's eye block is a bubble column.
    fn is_eye_in_bubble_column(&self) -> bool {
        let Some(world) = self.level() else {
            return false;
        };

        world
            .get_block_state(BlockPos::new(
                self.position().x.floor() as i32,
                self.get_eye_y().floor() as i32,
                self.position().z.floor() as i32,
            ))
            .get_block()
            == &vanilla_blocks::BUBBLE_COLUMN
    }

    /// Mirrors vanilla `LivingEntity.decreaseAirSupply`.
    fn decrease_air_supply(&self, current_supply: i32) -> i32 {
        let oxygen_bonus = self
            .attributes()
            .lock()
            .get_value(vanilla_attributes::OXYGEN_BONUS)
            .unwrap_or(0.0);
        if oxygen_bonus > 0.0 && rand::random::<f64>() >= 1.0 / (oxygen_bonus + 1.0) {
            current_supply
        } else {
            current_supply - 1
        }
    }

    /// Mirrors vanilla `LivingEntity.increaseAirSupply`.
    fn increase_air_supply(&self, current_supply: i32) -> i32 {
        (current_supply + 4).min(self.max_air_supply())
    }

    /// Mirrors vanilla `LivingEntity.shouldTakeDrowningDamage`.
    fn should_take_drowning_damage(&self) -> bool {
        self.air_supply() <= -20
    }

    /// Ticks vanilla living air-supply and drowning behavior from `baseTick`.
    fn tick_living_air_supply(&self) {
        if !LivingEntity::is_alive(self) {
            return;
        }

        let eye_in_water = self.is_eye_in_water() && !self.is_eye_in_bubble_column();
        if eye_in_water {
            if self.can_drown_in_water() {
                self.set_air_supply(self.decrease_air_supply(self.air_supply()));
                if self.should_take_drowning_damage() {
                    self.set_air_supply(0);
                    self.broadcast_entity_event(EntityStatus::DrownParticles);
                    if let Some(world) = self.level() {
                        self.hurt(
                            &world,
                            &DamageSource::environment(&vanilla_damage_types::DROWN),
                            2.0,
                        );
                    }
                }
            } else if self.air_supply() < self.max_air_supply()
                && self.should_effects_refill_air_supply()
            {
                self.set_air_supply(self.increase_air_supply(self.air_supply()));
            }

            if self
                .vehicle()
                .is_some_and(|vehicle| vehicle.dismounts_underwater())
            {
                self.stop_riding();
            }
            return;
        }

        if self.air_supply() < self.max_air_supply() {
            self.set_air_supply(self.increase_air_supply(self.air_supply()));
        }
    }

    /// Mirrors vanilla `LivingEntity.isInWall`.
    fn is_in_wall(&self) -> bool {
        !self.is_sleeping() && Entity::is_in_wall(self)
    }

    /// Applies vanilla living in-wall damage from `baseTick`.
    fn tick_in_wall_damage(&self) {
        if !LivingEntity::is_alive(self) || !LivingEntity::is_in_wall(self) {
            return;
        }

        if let Some(world) = self.level() {
            self.hurt(
                &world,
                &DamageSource::environment(&vanilla_damage_types::IN_WALL),
                1.0,
            );
        }
    }

    /// Applies vanilla living environmental damage in `LivingEntity.baseTick` order.
    fn tick_living_environmental_damage(&self) {
        if !LivingEntity::is_alive(self) {
            return;
        }

        if LivingEntity::is_in_wall(self) {
            self.tick_in_wall_damage();
        } else if self.as_player().is_some()
            && let Some(world) = self.level()
        {
            let border = world.world_border_snapshot();
            let position = self.position();
            if let Some(damage) =
                border.outside_damage_amount(position.x, position.z, self.bounding_box())
            {
                self.hurt(
                    &world,
                    &DamageSource::environment(&vanilla_damage_types::OUTSIDE_BORDER),
                    damage,
                );
            }
        }

        self.tick_living_air_supply();
    }

    /// Returns vanilla `LivingEntity.isAffectedByFluids()`.
    fn is_affected_by_fluids(&self) -> bool {
        true
    }

    /// Returns vanilla `LivingEntity.canStandOnFluid()`.
    fn can_stand_on_fluid(&self, _fluid_state: FluidState) -> bool {
        false
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

    /// Returns vanilla `LivingEntity.isHolding`.
    fn is_holding(&self, predicate: &mut dyn FnMut(&ItemStack) -> bool) -> bool {
        let mut holding = false;
        self.with_equipment_slot(EquipmentSlot::MainHand, &mut |item_stack| {
            holding = predicate(item_stack);
        });
        if holding {
            return true;
        }

        self.with_equipment_slot(EquipmentSlot::OffHand, &mut |item_stack| {
            holding = predicate(item_stack);
        });
        holding
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

    /// Returns whether this entity currently has an item in `slot`.
    fn has_item_in_slot(&self, slot: EquipmentSlot) -> bool {
        let mut has_item = false;
        self.with_equipment_slot(slot, &mut |item_stack| {
            has_item = !item_stack.is_empty();
        });
        has_item
    }

    /// Returns whether vanilla allows this entity to use `slot`.
    fn can_use_slot(&self, _slot: EquipmentSlot) -> bool {
        true
    }

    /// Returns the effective vanilla dispenser slot gate for living entities and mobs.
    fn can_dispenser_equip_into_slot(&self, _slot: EquipmentSlot) -> bool {
        self.as_mob().is_none_or(Mob::can_pick_up_loot)
    }

    /// Returns vanilla `LivingEntity.canEquipWithDispenser`.
    fn can_equip_with_dispenser(&self, item_stack: &ItemStack) -> bool {
        if !Entity::is_alive(self) || self.is_spectator() {
            return false;
        }

        let Some(equippable) = item_stack.get_equippable() else {
            return false;
        };
        if !equippable.dispensable {
            return false;
        }

        let slot = equippable.slot;
        self.can_use_slot(slot)
            && equippable.can_be_equipped_by(self.entity_type())
            && !self.has_item_in_slot(slot)
            && self.can_dispenser_equip_into_slot(slot)
    }

    /// Returns vanilla `LivingEntity.isEquippableInSlot`.
    fn is_equippable_in_slot(&self, item_stack: &ItemStack, slot: EquipmentSlot) -> bool {
        let Some(equippable) = item_stack.get_equippable() else {
            return slot == EquipmentSlot::MainHand && self.can_use_slot(EquipmentSlot::MainHand);
        };

        slot == equippable.slot
            && self.can_use_slot(equippable.slot)
            && equippable.can_be_equipped_by(self.entity_type())
    }

    /// Returns the equip sound Steel can currently resolve for this entity.
    fn equip_sound(&self, slot: EquipmentSlot, stack: &ItemStack) -> Option<SoundEventRef> {
        let equippable = stack.get_equippable()?;
        (slot == equippable.slot)
            .then(|| equippable.equip_sound.registry_ref())
            .flatten()
    }

    /// Runs vanilla's equippable `ItemStack.interactLivingEntity` branch.
    fn interact_living_entity_with_equippable(
        &self,
        player: &Player,
        hand: InteractionHand,
    ) -> InteractionResult {
        let item_stack = {
            let inventory = player.inventory.lock();
            let item_stack = inventory.get_item_in_hand(hand);
            item_stack.copy_with_count(item_stack.count())
        };
        let Some(equippable) = item_stack.get_equippable() else {
            return InteractionResult::Pass;
        };
        if !equippable.equip_on_interact {
            return InteractionResult::Pass;
        }

        let slot = equippable.slot;
        if !self.is_equippable_in_slot(&item_stack, slot) || !Entity::is_alive(self) {
            return InteractionResult::Pass;
        }

        let equipped = {
            let mut equipment = self.living_base().equipment().lock();
            if !equipment.get_ref(slot).is_empty() {
                return InteractionResult::Pass;
            }

            let mut inventory = player.inventory.lock();
            if !self.is_equippable_in_slot(inventory.get_item_in_hand(hand), slot) {
                return InteractionResult::Pass;
            }

            let equipped = inventory.split_item_in_hand(hand, 1);
            if equipped.is_empty() {
                return InteractionResult::Pass;
            }

            equipment.set(slot, equipped);
            equipment.get_ref(slot).copy_with_count(1)
        };

        self.refresh_equipment_attribute_modifiers(slot);
        if let Some(sound) = self.equip_sound(slot, &equipped) {
            self.play_sound(sound, 1.0, 1.0);
        }
        if let Some(mob) = self.as_mob() {
            mob.set_guaranteed_drop(slot);
        }
        // TODO: Emit EQUIP game event once game-event dispatch is implemented.
        InteractionResult::Success
    }

    /// Refreshes transient item attribute modifiers for one equipment slot.
    fn refresh_equipment_attribute_modifiers(&self, slot: EquipmentSlot) {
        self.with_equipment_slot(slot, &mut |item_stack| {
            self.living_base()
                .refresh_equipment_attribute_modifiers(slot, item_stack);
        });
    }

    /// Refreshes transient item attribute modifiers for all equipment slots.
    fn refresh_all_equipment_attribute_modifiers(&self) {
        for slot in EquipmentSlot::ALL {
            self.refresh_equipment_attribute_modifiers(slot);
        }
    }

    /// Packs non-empty living equipment slots for initial spawn pairing.
    fn pack_living_equipment(&self) -> Vec<EquipmentSlotItem> {
        equipment_items_to_packet_items(self.living_base().equipment().lock().non_empty_items())
    }

    /// Drains dirty living equipment slots for tracker sync.
    fn drain_dirty_living_equipment(&self) -> Vec<EquipmentSlotItem> {
        equipment_items_to_packet_items(self.living_base().equipment().lock().drain_dirty_items())
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
        for slot in EquipmentSlot::ALL {
            if !slot.is_armor() {
                continue;
            }
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

    /// Returns whether vanilla `tryAddFrost` sees a non-air block below.
    fn is_on_non_air_block_for_frost(&self) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let Some(pos) = self.on_pos_legacy() else {
            return false;
        };

        world.get_block_state(pos).get_block() != &vanilla_blocks::AIR
    }

    /// Mirrors vanilla `LivingEntity.removeFrost`.
    fn remove_frost(&self) {
        self.attributes().lock().remove_modifier(
            vanilla_attributes::MOVEMENT_SPEED,
            &SPEED_MODIFIER_POWDER_SNOW_ID,
        );
    }

    /// Mirrors vanilla `LivingEntity.tryAddFrost`.
    fn try_add_frost(&self) {
        if !self.is_on_non_air_block_for_frost() || self.ticks_frozen() <= 0 {
            return;
        }

        self.attributes().lock().add_modifier(
            vanilla_attributes::MOVEMENT_SPEED,
            AttributeModifier {
                id: SPEED_MODIFIER_POWDER_SNOW_ID,
                amount: f64::from(-0.05_f32 * self.percent_frozen()),
                operation: AttributeModifierOperation::AddValue,
            },
            false,
        );
    }

    /// Ticks vanilla `LivingEntity.aiStep` freezing effects.
    fn tick_freezing(&self) {
        if !self.is_in_powder_snow() || !self.can_freeze() {
            self.set_ticks_frozen((self.ticks_frozen() - 2).max(0));
        }

        self.remove_frost();
        self.try_add_frost();
        if self.tick_count() % 40 == 0
            && self.is_fully_frozen()
            && self.can_freeze()
            && let Some(world) = self.level()
        {
            self.hurt(
                &world,
                &DamageSource::environment(&vanilla_damage_types::FREEZE),
                1.0,
            );
        }
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
        if let Some(mob) = self.as_mob() {
            mob.tick_body_rotation_control();
        }
        self.living_base()
            .tick_fall_flying_state(self.is_fall_flying());
        self.update_swing_time();
        self.refresh_dirty_attributes();
        self.living_base().tick_post_impulse_grace_time();
        self.living_base().tick_last_hurt_by_player_memory();
        self.living_base()
            .tick_living_combat_memory(self.tick_count());
    }

    /// Mirrors vanilla `LivingEntity.canGlideUsing()`.
    fn can_glide_using(&self, item_stack: &ItemStack, slot: EquipmentSlot) -> bool {
        let Some(equippable) = item_stack.get_equippable() else {
            return false;
        };

        item_stack.has(GLIDER) && equippable.slot == slot && !item_stack.next_damage_will_break()
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

        let slot_index = rand::random_range(0..slot_count);
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

    /// Mirrors vanilla `LivingEntity.tickRidden()`.
    fn tick_ridden(&self, _controller: &Player, _ridden_input: DVec3) {}

    /// Mirrors vanilla `LivingEntity.getRiddenInput()`.
    fn ridden_input(&self, _controller: &Player, self_input: DVec3) -> DVec3 {
        self_input
    }

    /// Mirrors vanilla `LivingEntity.getRiddenSpeed()`.
    fn ridden_speed(&self, _controller: &Player) -> f32 {
        self.get_speed()
    }

    /// Mirrors vanilla `LivingEntity.travelRidden()`.
    fn travel_ridden(&self, controller: &Player, self_input: DVec3) -> Option<MoveResult> {
        let ridden_input = self.ridden_input(controller, self_input);
        self.tick_ridden(controller, ridden_input);
        if self.can_simulate_movement() {
            self.set_speed(self.ridden_speed(controller));
            return self.travel(ridden_input);
        }

        self.set_velocity(DVec3::ZERO);
        None
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

        if self.is_fall_flying() {
            self.update_fall_flying();
        }

        if self.has_mob_effect(vanilla_mob_effects::SLOW_FALLING)
            || self.has_mob_effect(vanilla_mob_effects::LEVITATION)
        {
            self.reset_fall_distance();
        }

        let input = self.travel_input();
        let input = DVec3::new(
            f64::from(input.sideways()),
            f64::from(input.vertical()),
            f64::from(input.forward()),
        );
        let result = if Entity::is_alive(self)
            && let Some(controller_entity) = self.controlling_passenger()
            && let Some(controller) = controller_entity.as_player()
        {
            self.travel_ridden(controller, input)
        } else if self.can_simulate_movement() && self.is_effective_ai() {
            self.travel(input)
        } else {
            None
        };

        self.apply_effects_from_blocks();
        self.tick_freezing();
        self.push_entities();
        result
    }

    /// Mirrors vanilla `LivingEntity.aiStep()`.
    fn ai_step(&self) -> Option<MoveResult> {
        self.default_ai_step()
    }

    /// Mirrors vanilla `LivingEntity.pushEntities()`.
    fn push_entities(&self) {
        let Some(world) = self.level() else {
            return;
        };
        if !world.tick_runs_normally() {
            return;
        }

        let pusher = self.as_entity_event_source();
        let pushable_entities = world.get_pushable_entities(pusher, &self.bounding_box());
        if pushable_entities.is_empty() {
            return;
        }

        self.apply_entity_cramming_damage(&world, &pushable_entities);

        for entity in pushable_entities {
            entity.push_entity(pusher);
        }
    }

    /// Applies vanilla max entity cramming damage from `LivingEntity.pushEntities()`.
    fn apply_entity_cramming_damage(&self, world: &World, pushable_entities: &[SharedEntity]) {
        let max_cramming = world
            .get_game_rule(&MAX_ENTITY_CRAMMING)
            .as_int()
            .unwrap_or(24);

        if max_cramming <= 0 || pushable_entities.len() <= (max_cramming - 1) as usize {
            return;
        }

        let random_roll = rand::random_range(0..4);
        let non_passenger_count = pushable_entities
            .iter()
            .filter(|entity| !entity.is_passenger())
            .count();

        if should_apply_entity_cramming_damage(
            max_cramming,
            pushable_entities.len(),
            non_passenger_count,
            random_roll,
        ) {
            self.hurt(
                world,
                &DamageSource::environment(&vanilla_damage_types::CRAMMING),
                6.0,
            );
        }
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
        if let Some(world) = self.level() {
            self.hurt(
                &world,
                &DamageSource::environment(&vanilla_damage_types::FLY_INTO_WALL),
                damage,
            );
        }
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

    /// Mirrors vanilla `LivingEntity.setIgnoreFallDamageFromCurrentImpulse`.
    fn set_ignore_fall_damage_from_current_impulse(
        &self,
        ignore_fall_damage: bool,
        new_impulse_impact_pos: DVec3,
    ) {
        self.living_base()
            .set_ignore_fall_damage_from_current_impulse(
                ignore_fall_damage,
                new_impulse_impact_pos,
            );
    }

    /// Returns vanilla `LivingEntity.isIgnoringFallDamageFromCurrentImpulse`.
    fn is_ignoring_fall_damage_from_current_impulse(&self) -> bool {
        self.living_base()
            .is_ignoring_fall_damage_from_current_impulse()
    }

    /// Returns vanilla `LivingEntity.currentImpulseImpactPos`.
    fn current_impulse_impact_pos(&self) -> Option<DVec3> {
        self.living_base().current_impulse_impact_pos()
    }

    /// Mirrors vanilla `LivingEntity.tryResetCurrentImpulseContext`.
    fn try_reset_current_impulse_context(&self) {
        self.living_base().try_reset_current_impulse_context();
    }

    /// Mirrors vanilla `LivingEntity.resetCurrentImpulseContext`.
    fn reset_current_impulse_context(&self) {
        self.living_base().reset_current_impulse_context();
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
            } else if attr.key == vanilla_attributes::SCALE.key {
                self.refresh_dimensions();
            }
            // TODO: WAYPOINT_TRANSMIT_RANGE → waypoint manager
        }
    }
}

fn death_loot_items_with_rng<R: rand::Rng, E: LivingEntity + ?Sized>(
    entity: &E,
    loot_table: LootTableRef,
    world: &World,
    source: &DamageSource,
    killed_by_player: bool,
    rng: &mut R,
) -> Vec<ItemStack> {
    let causing_entity = source
        .causing_entity_id
        .and_then(|entity_id| world.get_entity_by_id(entity_id));
    let direct_entity = source
        .direct_entity_id
        .and_then(|entity_id| world.get_entity_by_id(entity_id));
    let last_damage_player = if killed_by_player {
        entity
            .last_hurt_by_player_uuid()
            .and_then(|uuid| world.get_entity_by_uuid(&uuid))
    } else {
        None
    };

    let position = entity.position();
    let this_entity = living_entity_loot_ref(entity);
    let causing_entity = causing_entity.as_deref().map(entity_loot_ref);
    let direct_entity = direct_entity.as_deref().map(entity_loot_ref);
    let last_damage_player = last_damage_player.as_deref().map(entity_loot_ref);
    let damage_source = DamageSourceInfo {
        damage_type: Some(&source.damage_type.key),
        tags: &[],
        is_direct: source.is_direct(),
    };

    let mut context = LootContext::new(rng)
        .with_origin(position.x, position.y, position.z)
        .with_game_time(world.game_time())
        .with_killed_by_player(killed_by_player)
        .with_this_entity(this_entity)
        .with_damage_source(damage_source);
    if let Some(entity) = causing_entity {
        context = context.with_killer_entity(entity);
    }
    if let Some(entity) = direct_entity {
        context = context.with_direct_killer_entity(entity);
    }
    if let Some(entity) = last_damage_player {
        context = context.with_last_damage_player(entity);
    }

    loot_table.get_random_items(&mut context)
}

fn living_entity_loot_ref<E: LivingEntity + ?Sized>(entity: &E) -> EntityRef<'_> {
    EntityRef {
        entity_type: Some(&entity.entity_type().key),
        flags: EntityRefFlags {
            is_on_fire: entity.is_on_fire(),
            is_sneaking: entity.is_crouching(),
            is_sprinting: entity.is_sprinting(),
            is_swimming: entity.is_swimming(),
            is_baby: entity.is_baby(),
        },
        // TODO: Include equipment and custom name once loot contexts can snapshot entity data.
        equipment: None,
        custom_name: None,
    }
}

fn entity_loot_ref(entity: &dyn Entity) -> EntityRef<'_> {
    let living_entity = entity.as_living_entity();
    EntityRef {
        entity_type: Some(&entity.entity_type().key),
        flags: EntityRefFlags {
            is_on_fire: entity.is_on_fire(),
            is_sneaking: entity.is_crouching(),
            is_sprinting: living_entity.is_some_and(LivingEntity::is_sprinting),
            is_swimming: entity.is_swimming(),
            is_baby: living_entity.is_some_and(LivingEntity::is_baby),
        },
        // TODO: Include equipment and custom name once loot contexts can snapshot entity data.
        equipment: None,
        custom_name: None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
    use steel_protocol::packets::game::RelativeMovement;
    use steel_registry::blocks::{
        block_state_ext::BlockStateExt as _,
        properties::{BlockStateProperties, Direction as BlockDirection},
    };
    use steel_registry::entity_data::EntityPose;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::fluid::FluidState;
    use steel_registry::game_events::GameEventRef;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::vanilla_entity_data::LivingEntityData as SyncedLivingEntityData;
    use steel_registry::{
        REGISTRY, sound_events, test_support::init_test_registry, vanilla_attributes,
        vanilla_blocks, vanilla_damage_types, vanilla_entities, vanilla_fluids,
        vanilla_game_events, vanilla_items, vanilla_loot_tables, vanilla_mob_effects,
    };
    use steel_utils::Downcast as _;
    use steel_utils::locks::SyncMutex;
    use steel_utils::types::InteractionHand;
    use steel_utils::{
        BlockPos, BlockStateId, Direction, Identifier, SectionPos, WorldAabb, axis::Axis,
        block_util::FoundRectangle,
    };
    use text_components::{Modifier as _, TextComponent, format::Color, interactivity::ClickEvent};
    use uuid::Uuid;

    use crate::behavior::init_behaviors;
    use crate::chunk_saver::ChunkStorage;
    use crate::entity::damage::DamageSource;
    use crate::entity::entities::PigEntity;
    use crate::entity::mob::Mob;
    use crate::inventory::equipment::EquipmentSlot;
    use crate::portal::PortalKind;
    use crate::test_support::{cross_world_damage_test_world, test_world};
    use crate::world::game_event_context::GameEventContext;
    use crate::world::game_event_listener::{GameEventListener, SharedGameEventListener};
    use crate::world::{LevelReader, World};

    use super::{
        ActiveMobEffect, AttributeModifier, AttributeModifierOperation, DAMAGE_KNOCKBACK_POWER,
        DEFAULT_SWING_DURATION, DEFAULT_TICKS_REQUIRED_TO_FREEZE, Entity, EntityBase,
        EntityFluidContact, EntityLevelCallback, EntityMoveError, EntitySyncedData,
        EntityVerticalMovementStateUpdate, InsideBlockEffectType, LivingEntity, LivingEntityBase,
        LivingTravelInput, RemovalReason, SPEED_MODIFIER_POWDER_SNOW_ID, SharedEntity,
        block_state_suffocates_eye_box, closest_open_space_direction,
        fall_damage_reset_clip_target, fall_flying_collision_damage,
        fall_flying_free_fall_interval, get_input_vector, indirect_passengers,
        passenger_transition_position, passenger_transition_rotation,
        remove_after_changing_dimensions, should_apply_entity_cramming_damage,
        should_apply_resolved_movement, start_riding_entities, transfer_leashables_to_holder,
        trapdoor_usable_as_ladder_state,
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

    crate::entity::impl_test_downcast_type!(PushableTestEntity);

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

    struct TypedTestEntity {
        base: EntityBase,
        entity_type: EntityTypeRef,
        projectile_owner_uuid: Option<Uuid>,
    }

    impl TypedTestEntity {
        fn new(id: i32, entity_type: EntityTypeRef) -> Self {
            Self {
                base: EntityBase::new(id, DVec3::ZERO, entity_type.dimensions, Weak::new()),
                entity_type,
                projectile_owner_uuid: None,
            }
        }

        fn projectile_with_owner_uuid(id: i32, owner_uuid: Uuid) -> Self {
            Self {
                base: EntityBase::new(
                    id,
                    DVec3::ZERO,
                    vanilla_entities::ENDER_PEARL.dimensions,
                    Weak::new(),
                ),
                entity_type: &vanilla_entities::ENDER_PEARL,
                projectile_owner_uuid: Some(owner_uuid),
            }
        }
    }

    crate::entity::impl_test_downcast_type!(TypedTestEntity);

    impl Entity for TypedTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }

        fn projectile_owner_uuid(&self) -> Option<Uuid> {
            self.projectile_owner_uuid
        }
    }

    #[test]
    fn non_player_command_identity_uses_uuid_and_resolved_name() {
        let entity = TypedTestEntity::new(1, &vanilla_entities::PIG);

        assert_eq!(entity.scoreboard_name(), entity.uuid().to_string());
        assert_eq!(entity.plain_text_name(), "Pig");

        entity.set_custom_name(Some(TextComponent::plain("Command Pig")));
        assert_eq!(entity.plain_text_name(), "Command Pig");
    }

    #[test]
    fn entity_display_name_preserves_the_custom_name_component() {
        let entity = TypedTestEntity::new(1, &vanilla_entities::PIG);
        let custom_name = TextComponent::plain("Command Pig")
            .color(Color::Red)
            .click_event(ClickEvent::run_command("/root-action"))
            .add_child(
                TextComponent::plain(" Child")
                    .italic(true)
                    .click_event(ClickEvent::run_command("/child-action")),
            );
        entity.set_custom_name(Some(custom_name.clone()));

        let display_name = entity.display_name();
        let expected_insertion = entity.uuid().to_string();

        assert_eq!(display_name.content, custom_name.content);
        assert_eq!(display_name.format, custom_name.format);
        assert_eq!(display_name.children.len(), 1);
        assert_eq!(
            display_name.children[0].content,
            custom_name.children[0].content
        );
        assert_eq!(
            display_name.children[0].format,
            custom_name.children[0].format
        );
        assert!(display_name.interactions.click.is_none());
        assert!(
            display_name
                .children
                .iter()
                .all(|child| child.interactions.click.is_none())
        );
        assert_eq!(
            display_name.interactions.insertion.as_deref(),
            Some(expected_insertion.as_str())
        );
        assert!(display_name.interactions.hover.is_some());
    }

    #[test]
    fn command_data_compare_nbt_contains_base_and_custom_data() {
        let entity = TypedTestEntity::new(1, &vanilla_entities::PIG);
        entity.set_velocity(DVec3::new(0.25, -0.5, 0.75));
        entity.set_rotation((45.0, 10.0));
        entity.set_on_ground(true);
        entity.add_tag("selected".to_owned());
        let mut custom_data = NbtCompound::new();
        custom_data.insert("flag", NbtTag::Byte(1));
        entity.set_custom_data(custom_data);

        let nbt = entity.nbt_for_data_compare();

        assert_eq!(
            nbt.get("Motion"),
            Some(&NbtTag::List(NbtList::Double(vec![0.25, -0.5, 0.75])))
        );
        assert_eq!(
            nbt.get("Rotation"),
            Some(&NbtTag::List(NbtList::Float(vec![45.0, 10.0])))
        );
        assert_eq!(nbt.get("OnGround"), Some(&NbtTag::Byte(1)));
        assert_eq!(
            nbt.compound("data").and_then(|data| data.byte("flag")),
            Some(1)
        );
        assert!(matches!(
            nbt.get("Tags"),
            Some(NbtTag::List(NbtList::String(tags)))
                if tags.len() == 1 && tags[0].to_str() == "selected"
        ));
    }

    #[test]
    fn command_data_compare_nbt_contains_implemented_living_data() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true).with_health(12.5);
        entity.set_absorption_amount(3.0);
        entity.living_base.increment_death_time();
        entity.living_base.apply_post_impulse_grace_time(7);
        entity
            .living_base
            .set_ignore_fall_damage_from_current_impulse(true, DVec3::new(1.0, 2.0, 3.0));
        entity.set_fall_flying(true);
        entity.set_sleeping_pos(BlockPos::new(4, 5, 6));
        entity.add_mob_effect(
            ActiveMobEffect::with_duration(vanilla_mob_effects::HASTE, 200, 2)
                .with_ambient(true)
                .with_visible(false),
        );
        entity.equip(
            EquipmentSlot::Head,
            ItemStack::new(&vanilla_items::ITEMS.diamond_helmet),
        );

        let nbt = entity.nbt_for_data_compare();

        assert_eq!(nbt.get("Health"), Some(&NbtTag::Float(12.5)));
        assert_eq!(nbt.get("DeathTime"), Some(&NbtTag::Short(1)));
        assert_eq!(nbt.get("AbsorptionAmount"), Some(&NbtTag::Float(3.0)));
        assert_eq!(
            nbt.get("current_impulse_context_reset_grace_time"),
            Some(&NbtTag::Int(40))
        );
        assert_eq!(
            nbt.get("current_explosion_impact_pos"),
            Some(&NbtTag::List(NbtList::Double(vec![1.0, 2.0, 3.0])))
        );
        assert_eq!(nbt.get("FallFlying"), Some(&NbtTag::Byte(1)));
        assert_eq!(
            nbt.get("sleeping_pos"),
            Some(&NbtTag::IntArray(vec![4, 5, 6]))
        );

        let Some(NbtTag::List(NbtList::Compound(attributes))) = nbt.get("attributes") else {
            panic!("living attributes should be serialized");
        };
        assert!(attributes.iter().any(|attribute| {
            attribute.string("id").is_some_and(|id| {
                id.to_str().as_ref() == vanilla_attributes::MAX_HEALTH.key.to_string()
            })
        }));

        let Some(NbtTag::List(NbtList::Compound(effects))) = nbt.get("active_effects") else {
            panic!("active effects should be serialized");
        };
        assert_eq!(effects.len(), 1);
        assert_eq!(
            effects[0].string("id").map(ToString::to_string),
            Some("minecraft:haste".to_owned())
        );
        assert_eq!(effects[0].byte("amplifier"), Some(2));
        assert_eq!(effects[0].int("duration"), Some(200));
        assert_eq!(effects[0].byte("ambient"), Some(1));
        assert_eq!(effects[0].byte("show_particles"), Some(0));
        assert_eq!(effects[0].byte("show_icon"), Some(1));

        let Some(NbtTag::Compound(equipment)) = nbt.get("equipment") else {
            panic!("living equipment should be serialized");
        };
        assert_eq!(
            equipment
                .compound("head")
                .and_then(|item| item.string("id"))
                .map(ToString::to_string),
            Some("minecraft:diamond_helmet".to_owned())
        );
    }

    #[test]
    fn kill_uses_vanilla_living_and_non_living_paths() {
        let source_world = test_world();
        let target_world = cross_world_damage_test_world();
        assert!(!Arc::ptr_eq(source_world, target_world));
        let non_living_position = DVec3::new(0.25, 64.75, -0.125);
        let living_position = DVec3::new(1.25, 64.75, -0.125);
        let listener_position = DVec3::new(0.75, 64.75, -0.125);
        let listener_section = SectionPos::from_block_pos(BlockPos::from(listener_position));
        let target_listener = Arc::new(RecordingGameEventListener::new(listener_position));
        let target_shared_listener: SharedGameEventListener = target_listener.clone();
        let _target_registration = RegisteredGameEventListener::new(
            target_world,
            listener_section,
            Arc::clone(&target_shared_listener),
        );
        let source_listener = Arc::new(RecordingGameEventListener::new(listener_position));
        let source_shared_listener: SharedGameEventListener = source_listener.clone();
        let _source_registration = RegisteredGameEventListener::new(
            source_world,
            listener_section,
            Arc::clone(&source_shared_listener),
        );

        let non_living = TypedTestEntity::new(1, &vanilla_entities::ITEM);
        non_living.base().set_world(Arc::downgrade(target_world));
        non_living.base().set_position_local(non_living_position);
        non_living.kill(source_world);
        assert_eq!(non_living.removal_reason(), Some(RemovalReason::Killed));

        let living = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, target_world);
        living.base().set_position_local(living_position);
        living.kill(source_world);
        assert!(
            living
                .damage_types
                .lock()
                .iter()
                .any(|damage_type| damage_type == &vanilla_damage_types::GENERIC_KILL.key)
        );
        assert_eq!(
            living.damage_world_keys(),
            vec![source_world.key.to_string()]
        );
        assert_f32_close(living.get_health(), 0.0);
        assert_eq!(living.pose(), EntityPose::Dying);
        let Some(last_damage_source) = living.last_damage_source() else {
            panic!("kill damage should be timestamped in the victim world");
        };
        assert_eq!(
            last_damage_source.damage_type,
            &vanilla_damage_types::GENERIC_KILL
        );

        let events = target_listener.events.lock();
        assert_eq!(events.len(), 2);
        assert_eq!(
            matching_game_event_count(
                &events,
                &vanilla_game_events::ENTITY_DIE,
                non_living_position,
            ),
            1
        );
        assert_eq!(
            matching_game_event_count(&events, &vanilla_game_events::ENTITY_DIE, living_position),
            1
        );
        assert!(source_listener.events.lock().is_empty());
    }

    fn matching_game_event_count(
        events: &[(GameEventRef, DVec3)],
        expected_event: GameEventRef,
        expected_position: DVec3,
    ) -> usize {
        events
            .iter()
            .filter(|(event, position)| *event == expected_event && *position == expected_position)
            .count()
    }

    struct RegisteredGameEventListener<'a> {
        world: &'a Arc<World>,
        section: SectionPos,
        listener: SharedGameEventListener,
    }

    impl<'a> RegisteredGameEventListener<'a> {
        fn new(
            world: &'a Arc<World>,
            section: SectionPos,
            listener: SharedGameEventListener,
        ) -> Self {
            world.register_game_event_listener(section, Arc::clone(&listener));
            Self {
                world,
                section,
                listener,
            }
        }
    }

    impl Drop for RegisteredGameEventListener<'_> {
        fn drop(&mut self) {
            self.world
                .unregister_game_event_listener(self.section, &self.listener);
        }
    }

    struct RecordingGameEventListener {
        position: DVec3,
        events: SyncMutex<Vec<(GameEventRef, DVec3)>>,
    }

    impl RecordingGameEventListener {
        fn new(position: DVec3) -> Self {
            Self {
                position,
                events: SyncMutex::new(Vec::new()),
            }
        }
    }

    impl GameEventListener for RecordingGameEventListener {
        fn listener_pos(&self) -> Option<DVec3> {
            Some(self.position)
        }

        fn listener_radius(&self) -> i32 {
            16
        }

        fn handle_game_event(
            &self,
            _world: &Arc<World>,
            event: GameEventRef,
            _context: &GameEventContext<'_>,
            source_pos: DVec3,
        ) -> bool {
            self.events.lock().push((event, source_pos));
            true
        }
    }

    #[test]
    fn entity_downcast_uses_concrete_type_key_not_registry_type() {
        let entity = TypedTestEntity::new(1, &vanilla_entities::ITEM);
        let entity_ref: &dyn Entity = &entity;

        assert!(entity_ref.is::<TypedTestEntity>());
        assert!(entity_ref.downcast_ref::<TypedTestEntity>().is_some());
        assert!(entity_ref.downcast_ref::<PushableTestEntity>().is_none());
    }

    struct LeashNotificationTestEntity {
        base: EntityBase,
        holder_notifications: SyncMutex<Vec<i32>>,
        removed_notifications: SyncMutex<Vec<i32>>,
    }

    impl LeashNotificationTestEntity {
        fn new(id: i32) -> Arc<Self> {
            Self::with_position(id, DVec3::ZERO)
        }

        fn with_position(id: i32, position: DVec3) -> Arc<Self> {
            Arc::new(Self {
                base: EntityBase::new(id, position, vanilla_entities::ITEM.dimensions, Weak::new()),
                holder_notifications: SyncMutex::new(Vec::new()),
                removed_notifications: SyncMutex::new(Vec::new()),
            })
        }

        fn holder_notifications(&self) -> Vec<i32> {
            self.holder_notifications.lock().clone()
        }

        fn removed_notifications(&self) -> Vec<i32> {
            self.removed_notifications.lock().clone()
        }
    }

    crate::entity::impl_test_downcast_type!(LeashNotificationTestEntity);

    impl Entity for LeashNotificationTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ITEM
        }

        fn notify_leash_holder(&self, leashable: &dyn Entity) {
            self.holder_notifications.lock().push(leashable.id());
        }

        fn notify_leashee_removed(&self, leashable: &dyn Entity) {
            self.removed_notifications.lock().push(leashable.id());
        }
    }

    struct MultiPassengerTestEntity {
        base: EntityBase,
    }

    impl MultiPassengerTestEntity {
        fn shared(id: i32) -> SharedEntity {
            Arc::new(Self {
                base: EntityBase::new(
                    id,
                    DVec3::ZERO,
                    vanilla_entities::ITEM.dimensions,
                    Weak::new(),
                ),
            })
        }
    }

    crate::entity::impl_test_downcast_type!(MultiPassengerTestEntity);

    impl Entity for MultiPassengerTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ITEM
        }

        fn can_add_passenger(&self, _passenger: &dyn Entity) -> bool {
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

    crate::entity::impl_test_downcast_type!(KnownMovementTestEntity);

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
        entity_data: SyncMutex<SyncedLivingEntityData>,
        health: SyncMutex<f32>,
        damage_types: SyncMutex<Vec<Identifier>>,
        damage_world_keys: SyncMutex<Vec<String>>,
        entity_type: EntityTypeRef,
        affected_by_fluids: bool,
        can_stand_on_fluid: bool,
        vehicle: bool,
        on_non_air_block_for_frost: bool,
        in_wall_for_base_tick: bool,
        flying_player: bool,
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
                entity_data: SyncMutex::new(SyncedLivingEntityData::new()),
                health: SyncMutex::new(20.0),
                damage_types: SyncMutex::new(Vec::new()),
                damage_world_keys: SyncMutex::new(Vec::new()),
                entity_type: &vanilla_entities::PLAYER,
                affected_by_fluids,
                can_stand_on_fluid: false,
                vehicle: false,
                on_non_air_block_for_frost: false,
                in_wall_for_base_tick: false,
                flying_player: false,
            }
        }

        fn new_in_world(
            water_height: f64,
            lava_height: f64,
            affected_by_fluids: bool,
            world: &Arc<World>,
        ) -> Self {
            let entity = Self::new(water_height, lava_height, affected_by_fluids);
            entity.base.set_world(Arc::downgrade(world));
            entity
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

        const fn with_non_air_frost_block(mut self) -> Self {
            self.on_non_air_block_for_frost = true;
            self
        }

        const fn with_in_wall_for_base_tick(mut self) -> Self {
            self.in_wall_for_base_tick = true;
            self
        }

        const fn with_flying_player(mut self) -> Self {
            self.flying_player = true;
            self
        }

        fn with_health(self, health: f32) -> Self {
            *self.health.lock() = health;
            self
        }

        fn damage_type_keys(&self) -> Vec<Identifier> {
            self.damage_types.lock().clone()
        }

        fn damage_world_keys(&self) -> Vec<String> {
            self.damage_world_keys.lock().clone()
        }

        fn with_eye_in_water(self) -> Self {
            let contact = self.base.fluid_contact();
            self.base.set_fluid_contact(EntityFluidContact::from_parts(
                contact.water_height(),
                contact.lava_height(),
                true,
                contact.eye_in_lava(),
            ));
            self
        }

        fn equip(&self, slot: EquipmentSlot, stack: ItemStack) {
            self.living_base.equipment().lock().set(slot, stack);
        }
    }

    crate::entity::impl_test_downcast_type!(LivingFluidTestEntity);

    impl Entity for LivingFluidTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }

        fn as_living_entity(&self) -> Option<&dyn LivingEntity> {
            Some(self)
        }

        fn is_vehicle(&self) -> bool {
            self.vehicle
        }

        fn get_default_gravity(&self) -> f64 {
            LivingEntity::get_attribute_gravity(self)
        }

        fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
            self.damage_types
                .lock()
                .push(source.damage_type.key.clone());
            self.damage_world_keys.lock().push(world.key.to_string());
            LivingEntity::hurt_server(self, world, source, amount)
        }

        fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
            Some(&self.entity_data)
        }

        fn is_flying_player(&self) -> bool {
            self.flying_player
        }
    }

    impl LivingEntity for LivingFluidTestEntity {
        fn living_base(&self) -> &LivingEntityBase {
            &self.living_base
        }

        fn get_health(&self) -> f32 {
            *self.health.lock()
        }

        fn set_health(&self, health: f32) {
            *self.health.lock() = health.clamp(0.0, self.get_max_health());
        }

        fn is_affected_by_fluids(&self) -> bool {
            self.affected_by_fluids
        }

        fn can_stand_on_fluid(&self, _fluid_state: FluidState) -> bool {
            self.can_stand_on_fluid
        }

        fn is_on_non_air_block_for_frost(&self) -> bool {
            self.on_non_air_block_for_frost
        }

        fn is_in_wall(&self) -> bool {
            !self.is_sleeping() && (self.in_wall_for_base_tick || Entity::is_in_wall(self))
        }
    }

    struct ControlledVehicleTestEntity {
        base: EntityBase,
        controller: Option<SharedEntity>,
    }

    struct EmptyTestLevel;

    impl LevelReader for EmptyTestLevel {
        fn get_block_state(&self, _pos: BlockPos) -> BlockStateId {
            REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            15
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
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

    crate::entity::impl_test_downcast_type!(ControlledVehicleTestEntity);

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

    fn assert_f32_close(left: f32, right: f32) {
        assert!(
            (left - right).abs() <= f32::EPSILON,
            "expected {left} to equal {right}"
        );
    }

    fn assert_f64_close(left: f64, right: f64) {
        assert!(
            (left - right).abs() <= 1.0e-12,
            "expected {left} to equal {right}"
        );
    }

    #[test]
    fn living_relative_portal_position_resets_forward_offset() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity
            .base()
            .set_position_local(DVec3::new(12.0, 66.0, 20.75));
        let portal_area = FoundRectangle {
            min_corner: BlockPos::new(10, 64, 20),
            axis1_size: 4,
            axis2_size: 5,
        };
        let dimensions = entity.dimensions_for_pose(entity.pose());

        assert_vec3_close(
            entity.get_relative_portal_position(Axis::X, portal_area),
            DVec3::new(
                0.5,
                2.0 / (f64::from(portal_area.axis2_size) - f64::from(dimensions.height)),
                0.0,
            ),
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
        entity.base().set_portal_cooldown(2);

        entity.default_tick();

        assert_eq!(entity.base().boarding_cooldown(), 1);
        assert_eq!(entity.base().portal_cooldown(), 1);
    }

    #[test]
    fn can_use_portal_requires_alive_entity() {
        let entity = PushableTestEntity::shared(1, DVec3::ZERO);
        assert!(entity.can_use_portal(false));

        entity.set_removed(RemovalReason::Discarded);

        assert!(!entity.can_use_portal(true));
    }

    #[test]
    fn static_vanilla_portal_overrides_reject_special_entities() {
        let fishing_hook = TypedTestEntity::new(1, &vanilla_entities::FISHING_BOBBER);
        let dragon = TypedTestEntity::new(2, &vanilla_entities::ENDER_DRAGON);
        let wither = TypedTestEntity::new(3, &vanilla_entities::WITHER);

        assert!(!fishing_hook.can_use_portal(true));
        assert!(!dragon.can_use_portal(true));
        assert!(!wither.can_use_portal(true));
    }

    #[test]
    fn projectile_owner_uuid_reports_projectile_owner_identity() {
        let owner_uuid = Uuid::from_u128(42);
        let pearl = TypedTestEntity::projectile_with_owner_uuid(1, owner_uuid);
        let no_player_owner = TypedTestEntity::new(3, &vanilla_entities::ENDER_PEARL);

        assert_eq!(pearl.projectile_owner_uuid(), Some(owner_uuid));
        assert_eq!(no_player_owner.projectile_owner_uuid(), None);
    }

    #[test]
    fn can_use_portal_respects_passenger_gate() {
        init_test_registry();

        let passenger = PushableTestEntity::shared(1, DVec3::ZERO);
        let vehicle = PushableTestEntity::shared(2, DVec3::ZERO);
        assert!(start_riding_entities(&passenger, &vehicle));

        assert!(!passenger.can_use_portal(false));
        assert!(passenger.can_use_portal(true));
    }

    #[test]
    fn indirect_passengers_match_vanilla_preorder() {
        let vehicle = MultiPassengerTestEntity::shared(1);
        let first = MultiPassengerTestEntity::shared(2);
        let second = MultiPassengerTestEntity::shared(3);
        let nested = MultiPassengerTestEntity::shared(4);

        EntityBase::restore_passenger_relationship(&vehicle, &first);
        EntityBase::restore_passenger_relationship(&vehicle, &second);
        EntityBase::restore_passenger_relationship(&first, &nested);

        let passenger_ids = indirect_passengers(vehicle.as_ref())
            .into_iter()
            .map(|passenger| passenger.id())
            .collect::<Vec<_>>();

        assert_eq!(passenger_ids, vec![2, 4, 3]);
    }

    #[test]
    fn passenger_transition_rotation_matches_vanilla_relative_flags() {
        let vehicle_rotation = (30.0, 10.0);
        let passenger_rotation = (70.0, -5.0);

        assert_eq!(
            passenger_transition_rotation(
                (90.0, 20.0),
                RelativeMovement::NONE,
                vehicle_rotation,
                passenger_rotation,
            ),
            (130.0, 5.0),
        );
        assert_eq!(
            passenger_transition_rotation(
                (15.0, -3.0),
                RelativeMovement::ROTATION,
                vehicle_rotation,
                passenger_rotation,
            ),
            (15.0, -3.0),
        );
        assert_eq!(
            passenger_transition_rotation(
                (-90.0, 0.0),
                RelativeMovement::new(RelativeMovement::X_ROT),
                vehicle_rotation,
                passenger_rotation,
            ),
            (-50.0, 0.0),
        );
    }

    #[test]
    fn passenger_transition_position_preserves_vehicle_offset() {
        assert_eq!(
            passenger_transition_position(
                DVec3::new(100.0, 70.0, -40.0),
                RelativeMovement::NONE,
                DVec3::new(10.0, 64.0, 20.0),
                DVec3::new(12.5, 65.0, 17.0),
            ),
            DVec3::new(102.5, 71.0, -43.0),
        );
    }

    #[test]
    fn dimension_transition_persistence_keeps_non_chunk_serializable_entities() {
        let entity: SharedEntity =
            Arc::new(TypedTestEntity::new(1, &vanilla_entities::FISHING_BOBBER));
        entity
            .base()
            .set_position_local(DVec3::new(12.25, 64.0, -8.75));
        entity.set_rotation((45.0, -10.0));
        entity.set_velocity(DVec3::new(0.1, 0.2, 0.3));

        assert!(ChunkStorage::entity_tree_to_persistent(&entity).is_none());
        let persistent = ChunkStorage::entity_to_dimension_transition_persistent(&entity).expect(
            "dimension transitions mirror vanilla saveWithoutId without chunk-save filtering",
        );

        assert_eq!(persistent.entity_type, vanilla_entities::FISHING_BOBBER.key);
        assert_eq!(
            persistent.pos.map(f64::to_bits),
            [12.25_f64, 64.0, -8.75].map(f64::to_bits),
        );
        assert_eq!(
            persistent.rotation.map(f32::to_bits),
            [45.0_f32, -10.0].map(f32::to_bits),
        );
        assert_eq!(
            persistent.motion.map(f64::to_bits),
            [0.1_f64, 0.2, 0.3].map(f64::to_bits),
        );
    }

    #[test]
    fn remove_after_changing_dimensions_clears_old_mob_leash_and_equipment() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let holder: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::new(1.0, 0.0, 0.0),
            Weak::new(),
        ));
        let Some(mob) = pig.as_mob() else {
            panic!("pig should expose mob behavior");
        };
        assert!(mob.set_leashed_to(&holder));
        pig.living_base().equipment().lock().set(
            EquipmentSlot::Saddle,
            ItemStack::new(&vanilla_items::ITEMS.saddle),
        );

        remove_after_changing_dimensions(&pig);

        assert!(!mob.is_leashed());
        assert!(
            pig.living_base()
                .equipment()
                .lock()
                .get_ref(EquipmentSlot::Saddle)
                .is_empty()
        );
    }

    #[test]
    fn can_use_portal_rejects_sleeping_living_entities() {
        init_test_registry();

        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        assert!(entity.can_use_portal(false));

        entity.set_sleeping_pos(BlockPos::ZERO);

        assert!(!entity.can_use_portal(false));
    }

    #[test]
    fn dimension_changing_delay_uses_vanilla_class_overrides() {
        let base = TypedTestEntity::new(1, &vanilla_entities::ITEM);
        assert_eq!(base.dimension_changing_delay(), 300);

        let minecart = TypedTestEntity::new(2, &vanilla_entities::MINECART);
        assert_eq!(minecart.dimension_changing_delay(), 10);

        let arrow = TypedTestEntity::new(3, &vanilla_entities::ARROW);
        assert_eq!(arrow.dimension_changing_delay(), 2);
    }

    #[test]
    fn set_as_inside_portal_starts_portal_process_when_not_on_cooldown() {
        let entity = TypedTestEntity::new(1, &vanilla_entities::ITEM);
        let entry_position = BlockPos::new(2, 64, 2);

        entity.set_as_inside_portal(PortalKind::Nether, entry_position);

        let process = entity.base().portal_process().expect("portal process");
        assert_eq!(process.portal(), PortalKind::Nether);
        assert_eq!(process.entry_position(), entry_position);
    }

    #[test]
    fn set_as_inside_portal_resets_cooldown_without_starting_process() {
        let entity = TypedTestEntity::new(1, &vanilla_entities::ARROW);
        entity.set_portal_cooldown(1);

        entity.set_as_inside_portal(PortalKind::Nether, BlockPos::new(2, 64, 2));

        assert_eq!(entity.portal_cooldown(), 2);
        assert_eq!(entity.base().portal_process(), None);
    }

    #[test]
    fn living_tick_state_decrements_last_hurt_by_player_memory() {
        init_test_registry();

        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        let player_uuid = Uuid::from_u128(42);
        entity.set_last_hurt_by_player(player_uuid, 1);

        entity.tick_living_state();

        assert_eq!(
            entity.living_base().last_hurt_by_player_uuid(),
            Some(player_uuid)
        );
        assert_eq!(entity.last_hurt_by_player_memory_time(), 0);

        entity.tick_living_state();

        assert!(entity.living_base().last_hurt_by_player_uuid().is_none());
        assert_eq!(entity.last_hurt_by_player_memory_time(), 0);
    }

    #[test]
    fn living_tick_state_updates_swing_time() {
        init_test_registry();

        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.swing(InteractionHand::MainHand, false);
        assert_eq!(entity.living_swing_state().swing_time(), -1);

        entity.tick_living_state();

        let swing = entity.living_swing_state();
        assert!(swing.swinging());
        assert_eq!(swing.swing_time(), 0);
        assert_eq!(swing.attack_anim().to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn current_swing_duration_uses_vanilla_dig_effects() {
        init_test_registry();

        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        assert_eq!(entity.current_swing_duration(), DEFAULT_SWING_DURATION);

        entity.set_mob_effect(vanilla_mob_effects::MINING_FATIGUE, 2);
        assert_eq!(entity.current_swing_duration(), DEFAULT_SWING_DURATION + 6);

        entity.set_mob_effect(vanilla_mob_effects::HASTE, 1);
        assert_eq!(entity.current_swing_duration(), DEFAULT_SWING_DURATION - 2);
    }

    #[test]
    fn living_combat_memory_stores_and_expires_last_hurt_by_mob() {
        init_test_registry();

        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        let attacker: SharedEntity = Arc::new(LivingFluidTestEntity::new(0.0, 0.0, true));
        entity.advance_tick_count();

        entity.set_last_hurt_by_mob(Some(&attacker));

        let Some(stored_attacker) = entity.last_hurt_by_mob() else {
            panic!("last hurt-by mob should be stored");
        };
        assert_eq!(stored_attacker.uuid(), attacker.uuid());
        assert_eq!(entity.last_hurt_by_mob_timestamp(), 1);

        entity.living_base().tick_living_combat_memory(101);
        assert!(entity.last_hurt_by_mob().is_some());

        entity.living_base().tick_living_combat_memory(102);
        assert!(entity.last_hurt_by_mob().is_none());
        assert_eq!(entity.last_hurt_by_mob_timestamp(), 102);
    }

    #[test]
    fn living_combat_memory_clears_dead_last_hurt_mob() {
        init_test_registry();

        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        let target = Arc::new(LivingFluidTestEntity::new(0.0, 0.0, true));
        let target_entity: SharedEntity = target.clone();

        entity.set_last_hurt_mob(Some(&target_entity));
        assert!(entity.last_hurt_mob().is_some());
        assert_eq!(entity.last_hurt_mob_timestamp(), 0);

        target.set_health(0.0);
        entity.living_base().tick_living_combat_memory(1);

        assert!(entity.last_hurt_mob().is_none());
        assert_eq!(entity.last_hurt_mob_timestamp(), 1);
    }

    #[test]
    fn living_death_loot_table_uses_default_and_custom_mob_tables() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let Some(default_table) = pig.death_loot_table() else {
            panic!("pig should resolve its default entity loot table");
        };
        assert_eq!(&default_table.key, &vanilla_loot_tables::ENTITIES_PIG.key);

        pig.set_death_loot_table(Some(Identifier::vanilla_static("entities/cow")));
        let Some(custom_table) = pig.death_loot_table() else {
            panic!("custom cow loot table should resolve");
        };
        assert_eq!(&custom_table.key, &vanilla_loot_tables::ENTITIES_COW.key);
        assert_eq!(LivingEntity::death_loot_table_seed(&pig), 0);

        pig.set_death_loot_table(Some(Identifier::vanilla_static("entities/not_real")));
        assert!(pig.death_loot_table().is_none());
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
    fn in_wall_eye_box_requires_suffocating_state_and_shape_overlap() {
        init_test_registry();
        init_behaviors();
        let pos = BlockPos::ZERO;
        let level = EmptyTestLevel;
        let inside_box = WorldAabb::new(0.1, 0.5, 0.1, 0.9, 0.500_001, 0.9);
        let outside_box = WorldAabb::new(1.1, 0.5, 0.1, 1.9, 0.500_001, 0.9);

        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let glass = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::GLASS);
        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);

        assert!(block_state_suffocates_eye_box(
            stone, &level, pos, inside_box
        ));
        assert!(!block_state_suffocates_eye_box(
            glass, &level, pos, inside_box
        ));
        assert!(!block_state_suffocates_eye_box(
            air, &level, pos, inside_box
        ));
        assert!(!block_state_suffocates_eye_box(
            stone,
            &level,
            pos,
            outside_box
        ));
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
    fn living_armor_cover_counts_non_empty_humanoid_armor_slots() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

        assert_f32_close(entity.get_armor_cover_percentage(), 0.0);

        entity.equip(
            EquipmentSlot::Head,
            ItemStack::new(&vanilla_items::ITEMS.stone),
        );
        entity.equip(
            EquipmentSlot::Feet,
            ItemStack::new(&vanilla_items::ITEMS.stone),
        );

        assert_f32_close(entity.get_armor_cover_percentage(), 0.5);
    }

    #[test]
    fn living_visibility_percent_uses_discrete_and_invisible_scaling() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

        assert_f64_close(entity.get_visibility_percent(None), 1.0);

        EntitySyncedData::set_base_invisible_flag(&entity.entity_data, true);

        let invisible_without_armor = 0.7 * f64::from(0.1_f32);
        assert_f64_close(entity.get_visibility_percent(None), invisible_without_armor);

        entity.set_shared_shift_key_down(true);

        assert_f64_close(
            entity.get_visibility_percent(None),
            0.8 * invisible_without_armor,
        );
    }

    #[test]
    fn living_visibility_percent_uses_matching_mob_head_disguise() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        let skeleton = LivingFluidTestEntity::new(0.0, 0.0, true)
            .with_entity_type(&vanilla_entities::SKELETON);

        entity.equip(
            EquipmentSlot::Head,
            ItemStack::new(&vanilla_items::ITEMS.skeleton_skull),
        );

        assert_f64_close(entity.get_visibility_percent(Some(&skeleton)), 0.5);
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
    fn living_freeze_immunity_uses_body_armor_equipment() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

        entity.equip(
            EquipmentSlot::Body,
            ItemStack::new(&vanilla_items::ITEMS.leather_horse_armor),
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
    fn living_freezing_decays_when_not_in_powder_snow() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_ticks_frozen(10);

        entity.tick_freezing();

        assert_eq!(entity.ticks_frozen(), 8);
    }

    #[test]
    fn living_freezing_keeps_ticks_while_in_powder_snow() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_ticks_frozen(10);
        entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);

        entity.tick_freezing();

        assert_eq!(entity.ticks_frozen(), 11);
    }

    #[test]
    fn living_freezing_adds_powder_snow_speed_modifier() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true).with_non_air_frost_block();
        entity.set_ticks_frozen(DEFAULT_TICKS_REQUIRED_TO_FREEZE / 2);
        entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);
        let base_speed = entity
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED);

        entity.tick_freezing();

        let attributes = entity.attributes().lock();
        assert!(attributes.has_modifier(
            vanilla_attributes::MOVEMENT_SPEED,
            &SPEED_MODIFIER_POWDER_SNOW_ID,
        ));
        assert_f64_close(
            attributes.required_value(vanilla_attributes::MOVEMENT_SPEED),
            base_speed - f64::from(0.05_f32 * entity.percent_frozen()),
        );
    }

    #[test]
    fn living_freezing_removes_stale_powder_snow_speed_modifier() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.attributes().lock().add_modifier(
            vanilla_attributes::MOVEMENT_SPEED,
            AttributeModifier {
                id: SPEED_MODIFIER_POWDER_SNOW_ID,
                amount: -0.05,
                operation: AttributeModifierOperation::AddValue,
            },
            false,
        );

        entity.tick_freezing();

        assert!(!entity.attributes().lock().has_modifier(
            vanilla_attributes::MOVEMENT_SPEED,
            &SPEED_MODIFIER_POWDER_SNOW_ID,
        ));
    }

    #[test]
    fn living_freezing_damages_fully_frozen_entities_on_frequency() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world());
        entity.set_ticks_frozen(DEFAULT_TICKS_REQUIRED_TO_FREEZE);
        entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);
        for _ in 0..40 {
            entity.advance_tick_count();
        }

        entity.tick_freezing();

        assert_f32_close(entity.get_health(), 19.0);
    }

    #[test]
    fn default_ai_step_ticks_freezing_after_travel() {
        init_test_registry();
        init_behaviors();
        let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world());
        entity.set_ticks_frozen(DEFAULT_TICKS_REQUIRED_TO_FREEZE);
        entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);
        for _ in 0..40 {
            entity.advance_tick_count();
        }

        entity.default_ai_step();

        assert_eq!(
            entity.damage_type_keys(),
            vec![vanilla_damage_types::FREEZE.key.clone()]
        );
        assert_f32_close(entity.get_health(), 19.0);
    }

    #[test]
    fn entity_cramming_damage_threshold_matches_vanilla_push_entities() {
        assert!(!should_apply_entity_cramming_damage(0, 100, 100, 0));
        assert!(!should_apply_entity_cramming_damage(24, 23, 23, 0));
        assert!(!should_apply_entity_cramming_damage(24, 24, 23, 0));
        assert!(!should_apply_entity_cramming_damage(24, 24, 24, 1));
        assert!(should_apply_entity_cramming_damage(24, 24, 24, 0));
    }

    #[test]
    fn freezing_damage_hurts_extra_tagged_entity_types() {
        init_test_registry();
        let entity =
            LivingFluidTestEntity::new(0.0, 0.0, true).with_entity_type(&vanilla_entities::BLAZE);

        assert!(entity.hurt(
            test_world(),
            &DamageSource::environment(&vanilla_damage_types::FREEZE),
            1.0,
        ));

        assert_f32_close(entity.get_health(), 15.0);
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
    fn living_fall_damage_uses_shared_damage_path_from_entity_dispatch() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world())
            .with_entity_type(&vanilla_entities::PIG);

        assert!(entity.cause_fall_damage(
            8.0,
            1.0,
            &DamageSource::environment(&vanilla_damage_types::FALL),
        ));

        assert_f32_close(entity.get_health(), 15.0);
    }

    #[test]
    fn living_fall_damage_caps_distance_from_current_impulse() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world());

        entity.set_ignore_fall_damage_from_current_impulse(true, DVec3::new(0.0, 4.0, 0.0));

        assert!(entity.cause_fall_damage(
            8.0,
            1.0,
            &DamageSource::environment(&vanilla_damage_types::FALL),
        ));

        assert_f32_close(entity.get_health(), 19.0);
        assert!(!entity.is_ignoring_fall_damage_from_current_impulse());
    }

    #[test]
    fn living_fall_damage_resets_current_impulse_when_landing_above_impact() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

        entity.set_ignore_fall_damage_from_current_impulse(true, DVec3::new(0.0, -1.0, 0.0));

        assert!(!entity.cause_fall_damage(
            8.0,
            1.0,
            &DamageSource::environment(&vanilla_damage_types::FALL),
        ));

        assert_f32_close(entity.get_health(), 20.0);
        assert!(!entity.is_ignoring_fall_damage_from_current_impulse());
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
    fn inside_bubble_column_pushes_up_and_resets_fall_distance() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_velocity(DVec3::new(0.1, 0.68, 0.2));
        entity.set_fall_distance(4.0);

        entity.on_inside_bubble_column(false);

        assert_vec3_close(entity.velocity(), DVec3::new(0.1, 0.7, 0.2));
        assert_f64_close(entity.fall_distance(), 0.0);
    }

    #[test]
    fn inside_bubble_column_drags_down_and_resets_fall_distance() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_velocity(DVec3::new(0.1, -0.28, 0.2));
        entity.set_fall_distance(4.0);

        entity.on_inside_bubble_column(true);

        assert_vec3_close(entity.velocity(), DVec3::new(0.1, -0.3, 0.2));
        assert_f64_close(entity.fall_distance(), 0.0);
    }

    #[test]
    fn above_bubble_column_uses_vanilla_stronger_velocity_limits() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_velocity(DVec3::new(0.1, 1.75, 0.2));
        entity.set_fall_distance(4.0);

        entity.on_above_bubble_column(false, BlockPos::ZERO);

        assert_vec3_close(entity.velocity(), DVec3::new(0.1, 1.8, 0.2));
        assert_f64_close(entity.fall_distance(), 4.0);
    }

    #[test]
    fn above_bubble_column_drag_down_uses_vanilla_stronger_velocity_limit() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_velocity(DVec3::new(0.1, -0.88, 0.2));

        entity.on_above_bubble_column(true, BlockPos::ZERO);

        assert_vec3_close(entity.velocity(), DVec3::new(0.1, -0.9, 0.2));
    }

    #[test]
    fn flying_players_ignore_bubble_column_entity_hooks() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true).with_flying_player();
        let velocity = DVec3::new(0.1, 0.2, 0.3);
        entity.set_velocity(velocity);
        entity.set_fall_distance(4.0);

        entity.on_inside_bubble_column(false);
        entity.on_above_bubble_column(false, BlockPos::ZERO);

        assert_vec3_close(entity.velocity(), velocity);
        assert_f64_close(entity.fall_distance(), 4.0);
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
    fn living_air_supply_decrements_while_eye_in_water() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.5, 0.0, true).with_eye_in_water();

        entity.set_air_supply(entity.max_air_supply());
        entity.tick_living_air_supply();

        assert_eq!(entity.air_supply(), entity.max_air_supply() - 1);
    }

    #[test]
    fn living_air_supply_drowning_damage_resets_air() {
        init_test_registry();
        let entity =
            LivingFluidTestEntity::new_in_world(0.5, 0.0, true, test_world()).with_eye_in_water();

        entity.set_air_supply(-19);
        entity.tick_living_air_supply();

        assert_eq!(entity.air_supply(), 0);
        assert_f32_close(entity.get_health(), 18.0);
    }

    #[test]
    fn water_breathing_refills_air_underwater() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.5, 0.0, true).with_eye_in_water();

        entity.set_air_supply(entity.max_air_supply() - 8);
        entity.set_mob_effect_active(vanilla_mob_effects::WATER_BREATHING, true);
        entity.tick_living_air_supply();

        assert_eq!(entity.air_supply(), entity.max_air_supply() - 4);
    }

    #[test]
    fn breath_of_the_nautilus_prevents_drowning_without_refilling_air() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.5, 0.0, true).with_eye_in_water();

        entity.set_air_supply(entity.max_air_supply() - 8);
        entity.set_mob_effect_active(vanilla_mob_effects::BREATH_OF_THE_NAUTILUS, true);
        entity.tick_living_air_supply();

        assert_eq!(entity.air_supply(), entity.max_air_supply() - 8);
        assert_f32_close(entity.get_health(), 20.0);
    }

    #[test]
    fn entity_type_can_breathe_underwater_refills_air() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.5, 0.0, true)
            .with_eye_in_water()
            .with_entity_type(&vanilla_entities::ZOMBIE);

        entity.set_air_supply(entity.max_air_supply() - 8);
        entity.tick_living_air_supply();

        assert_eq!(entity.air_supply(), entity.max_air_supply() - 4);
    }

    #[test]
    fn living_air_supply_refills_out_of_water() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

        entity.set_air_supply(entity.max_air_supply() - 8);
        entity.tick_living_air_supply();

        assert_eq!(entity.air_supply(), entity.max_air_supply() - 4);
    }

    #[test]
    fn living_base_tick_damages_entities_in_wall() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world())
            .with_in_wall_for_base_tick();

        entity.base_tick_living_entity();

        assert_f32_close(entity.get_health(), 19.0);
    }

    #[test]
    fn living_environmental_damage_applies_in_wall_before_drowning() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new_in_world(0.5, 0.0, true, test_world())
            .with_eye_in_water()
            .with_in_wall_for_base_tick();

        entity.set_air_supply(-19);
        entity.tick_living_environmental_damage();

        assert_eq!(
            entity.damage_type_keys(),
            vec![
                vanilla_damage_types::IN_WALL.key.clone(),
                vanilla_damage_types::DROWN.key.clone(),
            ]
        );
    }

    #[test]
    fn living_base_tick_skips_in_wall_damage_while_sleeping() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true).with_in_wall_for_base_tick();
        entity.set_sleeping_pos(BlockPos::ZERO);

        entity.base_tick_living_entity();

        assert_f32_close(entity.get_health(), 20.0);
    }

    #[test]
    fn generic_living_hurt_applies_health_damage() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        assert!(entity.hurt(test_world(), &source, 4.0));

        assert_f32_close(entity.get_health(), 16.0);
    }

    #[test]
    fn generic_living_hurt_ignores_fire_damage_with_fire_resistance() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_mob_effect(vanilla_mob_effects::FIRE_RESISTANCE, 0);
        let source = DamageSource::environment(&vanilla_damage_types::LAVA);

        assert!(!entity.hurt(test_world(), &source, 4.0));

        assert_f32_close(entity.get_health(), 20.0);
    }

    #[test]
    fn generic_living_hurt_processes_default_death_once() {
        init_test_registry();
        let entity =
            LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world()).with_health(3.0);
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        assert!(entity.hurt(test_world(), &source, 4.0));
        assert_f32_close(entity.get_health(), 0.0);
        assert_eq!(entity.pose(), EntityPose::Dying);
        assert!(!entity.hurt(test_world(), &source, 1.0));
    }

    #[test]
    fn generic_living_hurt_applies_source_position_knockback() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_on_ground(true);
        let source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_source_position(DVec3::new(1.0, 0.0, 0.0));

        assert!(entity.hurt(test_world(), &source, 4.0));

        assert_vec3_close(
            entity.velocity(),
            DVec3::new(-DAMAGE_KNOCKBACK_POWER, 0.4, 0.0),
        );
        assert!(entity.needs_velocity_sync());
    }

    #[test]
    fn as_living_entity_exposes_living_behavior_without_downcasting() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        let entity_ref: &dyn Entity = &entity;
        let Some(living) = entity_ref.as_living_entity() else {
            panic!("living test entity did not expose LivingEntity behavior");
        };

        assert_f32_close(living.get_health(), 20.0);

        let non_living = PushableTestEntity::shared(2, DVec3::ZERO);
        assert!(non_living.as_living_entity().is_none());
    }

    #[test]
    fn head_yaw_uses_living_head_rotation_only() {
        init_test_registry();
        let living = LivingFluidTestEntity::new(0.0, 0.0, true);
        living.set_rotation((35.0, 0.0));
        living.set_y_head_rot(120.0);

        assert_f32_close(Entity::head_yaw(&living), 120.0);

        let non_living = PushableTestEntity::shared(2, DVec3::ZERO);
        non_living.set_rotation((35.0, 0.0));
        assert_f32_close(non_living.head_yaw(), 0.0);
    }

    #[test]
    fn living_equipment_attribute_modifiers_refresh_for_slot() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        let (base_armor, base_toughness) = {
            let attributes = entity.attributes().lock();
            (
                attributes.required_value(vanilla_attributes::ARMOR),
                attributes.required_value(vanilla_attributes::ARMOR_TOUGHNESS),
            )
        };

        entity.equip(
            EquipmentSlot::Head,
            ItemStack::new(&vanilla_items::ITEMS.diamond_helmet),
        );
        LivingEntity::refresh_equipment_attribute_modifiers(&entity, EquipmentSlot::Head);

        {
            let attributes = entity.attributes().lock();
            assert_eq!(
                attributes
                    .required_value(vanilla_attributes::ARMOR)
                    .to_bits(),
                (base_armor + 3.0).to_bits()
            );
            assert_eq!(
                attributes
                    .required_value(vanilla_attributes::ARMOR_TOUGHNESS)
                    .to_bits(),
                (base_toughness + 2.0).to_bits()
            );
        }

        entity.equip(EquipmentSlot::Head, ItemStack::empty());
        LivingEntity::refresh_equipment_attribute_modifiers(&entity, EquipmentSlot::Head);

        let attributes = entity.attributes().lock();
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::ARMOR)
                .to_bits(),
            base_armor.to_bits()
        );
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::ARMOR_TOUGHNESS)
                .to_bits(),
            base_toughness.to_bits()
        );
    }

    #[test]
    fn generic_living_hurt_respects_no_knockback_damage_tag() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_on_ground(true);
        entity.set_velocity(DVec3::new(0.2, 0.3, -0.1));
        let initial_velocity = entity.velocity();
        let source = DamageSource::environment(&vanilla_damage_types::DROWN)
            .with_source_position(DVec3::new(1.0, 0.0, 0.0));

        assert!(entity.hurt(test_world(), &source, 4.0));

        assert_vec3_close(entity.velocity(), initial_velocity);
        assert!(!entity.needs_velocity_sync());
    }

    #[test]
    fn generic_living_hurt_scales_knockback_by_resistance() {
        init_test_registry();
        let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
        entity.set_on_ground(true);
        entity
            .attributes()
            .lock()
            .set_base_value(vanilla_attributes::KNOCKBACK_RESISTANCE, 0.5);
        let source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_source_position(DVec3::new(1.0, 0.0, 0.0));

        assert!(entity.hurt(test_world(), &source, 4.0));

        assert_vec3_close(
            entity.velocity(),
            DVec3::new(
                -DAMAGE_KNOCKBACK_POWER * 0.5,
                DAMAGE_KNOCKBACK_POWER * 0.5,
                0.0,
            ),
        );
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
    fn default_ai_step_resets_fall_distance_for_slow_falling_and_levitation() {
        init_test_registry();

        let slow_falling = LivingFluidTestEntity::new(0.0, 0.0, true);
        slow_falling.set_fall_distance(7.0);
        slow_falling.set_mob_effect_active(vanilla_mob_effects::SLOW_FALLING, true);
        slow_falling.default_ai_step();

        assert_f64_close(slow_falling.fall_distance(), 0.0);

        let levitating = LivingFluidTestEntity::new(0.0, 0.0, true);
        levitating.set_fall_distance(7.0);
        levitating.set_mob_effect_active(vanilla_mob_effects::LEVITATION, true);
        levitating.default_ai_step();

        assert_f64_close(levitating.fall_distance(), 0.0);
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
    fn start_riding_entities_links_passenger_and_vehicle() {
        init_test_registry();

        let passenger = PushableTestEntity::shared(1, DVec3::ZERO);
        let vehicle = PushableTestEntity::shared(2, DVec3::ZERO);

        assert!(start_riding_entities(&passenger, &vehicle));

        assert!(passenger.is_passenger());
        assert_eq!(passenger.vehicle().map(|entity| entity.id()), Some(2));
        assert!(vehicle.has_passenger(passenger.as_ref()));
        assert_eq!(vehicle.first_passenger().map(|entity| entity.id()), Some(1));
        assert_eq!(passenger.pose(), EntityPose::Standing);
    }

    #[test]
    fn transfer_leashables_to_holder_moves_valid_mobs() {
        init_test_registry();

        let old_holder: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        let new_holder: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::ZERO,
            Weak::new(),
        ));
        let leashable: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            3,
            DVec3::new(1.0, 0.0, 0.0),
            Weak::new(),
        ));
        let Some(mob) = leashable.as_mob() else {
            panic!("pig should expose mob behavior");
        };
        assert!(mob.set_leashed_to(&old_holder));

        assert!(transfer_leashables_to_holder(
            vec![Arc::clone(&leashable)],
            &new_holder
        ));

        let Some(holder) = mob.leash_holder() else {
            panic!("transferred mob should stay leashed");
        };
        assert_eq!(holder.id(), new_holder.id());
    }

    #[test]
    fn transfer_leashables_to_holder_skips_mobs_outside_snap_distance() {
        init_test_registry();

        let old_holder: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        let new_holder: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::ZERO,
            Weak::new(),
        ));
        let leashable: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            3,
            DVec3::new(20.0, 0.0, 0.0),
            Weak::new(),
        ));
        let Some(mob) = leashable.as_mob() else {
            panic!("pig should expose mob behavior");
        };
        assert!(mob.set_leashed_to(&old_holder));

        assert!(!transfer_leashables_to_holder(
            vec![Arc::clone(&leashable)],
            &new_holder
        ));

        let Some(holder) = mob.leash_holder() else {
            panic!("untransferred mob should stay leashed");
        };
        assert_eq!(holder.id(), old_holder.id());
    }

    #[test]
    fn set_leashed_to_notifies_replaced_holder() {
        init_test_registry();

        let old_holder_typed = LeashNotificationTestEntity::new(1);
        let old_holder: SharedEntity = old_holder_typed.clone();
        let new_holder_typed = LeashNotificationTestEntity::new(2);
        let new_holder: SharedEntity = new_holder_typed.clone();
        let leashable: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            3,
            DVec3::ZERO,
            Weak::new(),
        ));
        let Some(mob) = leashable.as_mob() else {
            panic!("pig should expose mob behavior");
        };

        assert!(mob.set_leashed_to(&old_holder));
        assert!(mob.set_leashed_to(&new_holder));

        assert_eq!(old_holder_typed.removed_notifications(), vec![3]);
        assert!(new_holder_typed.removed_notifications().is_empty());
    }

    #[test]
    fn tick_leash_notifies_live_holder() {
        init_test_registry();

        let holder_typed = LeashNotificationTestEntity::new(1);
        let holder: SharedEntity = holder_typed.clone();
        let leashable: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            3,
            DVec3::ZERO,
            Weak::new(),
        ));
        let Some(mob) = leashable.as_mob() else {
            panic!("pig should expose mob behavior");
        };
        assert!(mob.set_leashed_to(&holder));

        mob.tick_leash();

        assert_eq!(holder_typed.holder_notifications(), vec![3]);
        assert!(mob.is_leashed());
        assert!(holder_typed.removed_notifications().is_empty());
    }

    #[test]
    fn tick_leash_snaps_live_holder_past_snap_distance() {
        init_test_registry();

        let holder_typed =
            LeashNotificationTestEntity::with_position(1, DVec3::new(13.0, 0.0, 0.0));
        let holder: SharedEntity = holder_typed.clone();
        let leashable: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            3,
            DVec3::ZERO,
            Weak::new(),
        ));
        let Some(mob) = leashable.as_mob() else {
            panic!("pig should expose mob behavior");
        };
        assert!(mob.set_leashed_to(&holder));

        mob.tick_leash();

        assert_eq!(holder_typed.holder_notifications(), vec![3]);
        assert_eq!(holder_typed.removed_notifications(), vec![3]);
        assert!(!mob.is_leashed());
    }

    #[test]
    fn start_riding_entities_respects_boarding_cooldown() {
        init_test_registry();

        let passenger = PushableTestEntity::shared(1, DVec3::ZERO);
        let vehicle = PushableTestEntity::shared(2, DVec3::ZERO);
        passenger.base().set_boarding_cooldown(2);

        assert!(!start_riding_entities(&passenger, &vehicle));
        assert!(!passenger.is_passenger());
        assert!(!vehicle.is_vehicle());
    }

    #[test]
    fn start_riding_entities_rejects_vehicle_cycles() {
        init_test_registry();

        let root = PushableTestEntity::shared(1, DVec3::ZERO);
        let child = PushableTestEntity::shared(2, DVec3::ZERO);
        EntityBase::restore_passenger_relationship(&root, &child);

        assert!(!start_riding_entities(&root, &child));
        assert_eq!(child.vehicle().map(|entity| entity.id()), Some(1));
        assert_eq!(root.first_passenger().map(|entity| entity.id()), Some(2));
    }

    #[test]
    fn start_riding_entities_inserts_player_passenger_first() {
        init_test_registry();

        let vehicle = MultiPassengerTestEntity::shared(1);
        let mob_passenger = PushableTestEntity::shared(2, DVec3::ZERO);
        let player_passenger =
            KnownMovementTestEntity::shared(3, &vanilla_entities::PLAYER, DVec3::ZERO, DVec3::ZERO);

        assert!(start_riding_entities(&mob_passenger, &vehicle));
        assert!(start_riding_entities(&player_passenger, &vehicle));

        let passenger_ids = vehicle
            .passengers()
            .into_iter()
            .map(|entity| entity.id())
            .collect::<Vec<_>>();
        assert_eq!(passenger_ids, vec![3, 2]);
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
    fn controlled_vehicle_returns_direct_controlled_vehicle_not_root_vehicle() {
        init_test_registry();

        let passenger =
            KnownMovementTestEntity::shared(1, &vanilla_entities::PLAYER, DVec3::ZERO, DVec3::ZERO);
        let vehicle = ControlledVehicleTestEntity::shared(2, Some(Arc::clone(&passenger)));
        let root_vehicle = ControlledVehicleTestEntity::shared(3, None);

        assert!(start_riding_entities(&passenger, &vehicle));
        assert!(start_riding_entities(&vehicle, &root_vehicle));

        let Some(controlled_vehicle) = passenger.controlled_vehicle() else {
            panic!("passenger should directly control the middle vehicle");
        };
        let Some(root) = passenger.root_vehicle() else {
            panic!("passenger should have a root vehicle");
        };

        assert_eq!(controlled_vehicle.id(), vehicle.id());
        assert_eq!(root.id(), root_vehicle.id());
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
