//! This module contains entity-related traits and types.

use std::{
    any::try_as_dyn,
    borrow::Cow,
    sync::{Arc, LazyLock, Weak},
};

use glam::DVec3;
use rand::{SeedableRng as _, rngs::StdRng};
use rustc_hash::FxHashSet;
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
use steel_registry::data_components::vanilla_components::{
    GLIDER, SWING_ANIMATION, SwingAnimation,
};
use steel_registry::enchantment_effect::EnchantmentEffectComponent;
use steel_registry::entity_data::{DataValue, EntityPose, HumanoidArm};
use steel_registry::entity_type::{EntityAttachment, EntityDimensions, EntityTypeRef};
use steel_registry::fluid::{FluidState, FluidStateExt as _};
use steel_registry::game_events::GameEventRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
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
    BlockPos, BlockStateId, ChunkPos, Direction, Downcast as _, ErasedType, Identifier,
    UuidExt as _, WorldAabb, axis::Axis, block_util::FoundRectangle, text::DisplayResolutor,
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
mod combat_rules;
pub mod damage;
pub mod entities;
#[expect(
    clippy::module_inception,
    reason = "the entity module mirrors vanilla's Entity class and groups its implementation"
)]
mod entity;
mod fluid_contact;
#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/entities.rs"]
mod generated_entities;
mod inside_block_effects;
mod item_based_steering;
mod living_base;
mod living_entity;
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
pub(crate) use entity::apply_entity_look_at;
pub use entity::{
    AcceptedClientMovement, AcceptedClientMovementOutcome, Entity, EntityEventSource,
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
pub use living_entity::LivingEntity;
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
    EntityHitResult, Projectile, ProjectileBase, ProjectileDeflection, ProjectileEventSource,
    ProjectileHit, ThrowableItemProjectile, ThrowableProjectile, compute_margin,
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

pub(crate) fn entity_loot_ref(entity: &dyn Entity) -> EntityRef<'_> {
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
mod tests;
