//! Vanilla `AbstractMinecart`: rail vehicles moved by [`MinecartBehavior`].
//!
//! State lives in the [`AbstractMinecartBase`] component and the synced
//! `AbstractMinecart` data layer, behavior on the [`AbstractMinecart`] view;
//! per-type hooks are [`AbstractMinecartProps`].

use std::ops::Deref;
use std::sync::Arc;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_math::trig;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{Direction, RailShape};
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::AbstractMinecartEntityData;
use steel_registry::{vanilla_blocks, vanilla_entities};
use steel_utils::axis::Axis;
use steel_utils::block_util::FoundRectangle;
use steel_utils::locks::CheckedMutex;
use steel_utils::nbt::NbtValueInput as _;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, WorldAabb};

use super::minecart_behavior::MinecartBehavior;
use super::vehicle_entity::{self, Vehicle};
use crate::block_entity::block_state_nbt;
use crate::entity::component::definition::{
    self as entity_definition, BlockSpeedFactorCtx, Definition,
};
use crate::entity::component::pipeline::{Flow, Pipeline, Stage, StageId};
use crate::entity::component::props::entity_props;
use crate::entity::component::{Component, Components as _, Has};
use crate::entity::dismount_helper;
use crate::entity::prelude::*;
use crate::entity::{
    Entity, EntityMovementEmission, EntityProps, EntitySpawnError, Living, SharedEntity,
    apply_item_stack_components, create_entity_instance,
    reset_forward_direction_of_relative_portal_position,
};
use crate::physics::MoverType;
use crate::player::Player;
use crate::world::World;

const LOWERED_PASSENGER_ATTACHMENT: DVec3 = DVec3::ZERO;
const WATER_SLOWDOWN_FACTOR: f32 = 0.95;
const LAVA_FALL_DISTANCE_FACTOR: f64 = 0.5;
const MIN_PUSH_DISTANCE_SQUARED: f32 = 1.0e-4;
const MIN_COLLISION_ALIGNMENT: f32 = 0.8;
const GRAVITY_IN_WATER: f64 = 0.005;
const GRAVITY: f64 = 0.04;
const AIR_DRAG: f32 = 0.95;
const OFF_TRACK_GROUND_SLOWDOWN_FACTOR: f64 = 0.5;
const PUSH_FORCE: f32 = 0.1;
const COLLISION_VELOCITY_RETENTION: f64 = 0.2;
const COLLISION_PUSH_SCALE: f64 = 0.5;
const FURNACE_COLLISION_VELOCITY_RETENTION: f64 = 0.95;
const NON_MINECART_PUSH_DIVISOR: f64 = 4.0;
/// The widest a passenger counts as when looking for a dismount spot.
const MAX_DISMOUNT_REACH: f32 = 1.0;

struct MinecartState {
    on_rails: bool,
    flipped: bool,
}

/// The vanilla `AbstractMinecart` fields without synced data: whether it is
/// on rails and whether its movement-facing rotation is flipped.
pub struct AbstractMinecartBase {
    state: CheckedMutex<MinecartState>,
    hooks: &'static dyn AbstractMinecartHooks,
}

impl AbstractMinecartBase {
    /// A minecart off rails, with `hooks` describing its type.
    #[must_use]
    pub const fn new(hooks: &'static dyn AbstractMinecartHooks) -> Self {
        Self {
            state: CheckedMutex::new(MinecartState {
                on_rails: false,
                flipped: false,
            }),
            hooks,
        }
    }
}

// SAFETY: This key is owned by Steel and uniquely identifies `AbstractMinecartBase`.
unsafe impl DowncastType for AbstractMinecartBase {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:entity_component/abstract_minecart");
}

impl Component for AbstractMinecartBase {
    fn save(&self, owner: &dyn Entity, nbt: &mut NbtCompound) {
        let minecart = AbstractMinecart {
            entity: owner,
            base: self,
        };
        if let Some(display_state) = minecart.custom_display_block() {
            nbt.insert(
                "DisplayState",
                NbtTag::Compound(block_state_nbt::save(display_state)),
            );
        }
        let display_offset = minecart.display_offset();
        if display_offset != minecart.default_display_offset() {
            nbt.insert("DisplayOffset", display_offset);
        }
        nbt.insert("FlippedRotation", i8::from(minecart.is_flipped()));
        nbt.insert("HasTicked", i8::from(owner.is_first_tick()));
    }

    fn load(&self, owner: &dyn Entity, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let minecart = AbstractMinecart {
            entity: owner,
            base: self,
        };
        minecart
            .set_custom_display_block(nbt.compound("DisplayState").and_then(block_state_nbt::load));
        minecart.set_display_offset(
            nbt.int_value("DisplayOffset")
                .unwrap_or_else(|| minecart.default_display_offset()),
        );
        minecart.set_flipped(
            nbt.byte_value("FlippedRotation")
                .is_some_and(|value| value != 0),
        );
        owner.set_first_tick(nbt.byte_value("HasTicked").is_some_and(|value| value != 0));
    }
}

entity_props! {
    /// Per-type properties of a vanilla `AbstractMinecart`.
    pub struct AbstractMinecartProps<E>: AbstractMinecartHooks for AbstractMinecart
    where E: Parts => abstract_minecart {
        /// `AbstractMinecart.isRideable`: whether it picks up mobs it runs into.
        fn is_rideable() -> bool = is_rideable_default;
        /// `AbstractMinecart.isFurnace`.
        fn is_furnace() -> bool = is_furnace_default;
        /// `AbstractMinecart.activateMinecart`: contact with an activator rail.
        fn activate_minecart(world: &World, pos: BlockPos, powered: bool) = activate_minecart_default;
        /// `AbstractMinecart.getDefaultDisplayBlockState`.
        fn default_display_block_state() -> BlockStateId = default_display_block_state_default;
        /// `AbstractMinecart.getDefaultDisplayOffset`, in sixteenths of a block.
        fn default_display_offset() -> i32 = default_display_offset_default;
        /// `AbstractMinecart.getMaxSpeed`.
        fn max_speed(world: &World) -> f64 = max_speed_default;
    }
}

/// An entity seen as a vanilla `AbstractMinecart`.
#[derive(Clone, Copy)]
pub struct AbstractMinecart<'a> {
    entity: &'a dyn Entity,
    base: &'a AbstractMinecartBase,
}

impl<'a> AbstractMinecart<'a> {
    /// Pairs a concrete minecart with its state.
    #[must_use]
    pub fn new<E: Entity + Has<AbstractMinecartBase>>(entity: &'a E) -> Self {
        Self {
            entity,
            base: entity.get::<AbstractMinecartBase>(),
        }
    }

    /// Returns `entity` as a minecart, if it is one.
    #[must_use]
    pub fn of(entity: &'a dyn Entity) -> Option<Self> {
        Some(Self {
            entity,
            base: entity.query::<AbstractMinecartBase>()?,
        })
    }

    /// Returns the entity this view belongs to.
    #[must_use]
    pub const fn entity(&self) -> &'a dyn Entity {
        self.entity
    }

    fn hooks(&self) -> &'static dyn AbstractMinecartHooks {
        self.base.hooks
    }

    /// Returns this minecart as a vehicle.
    #[must_use]
    pub fn vehicle_entity(&self) -> Vehicle<'a> {
        Vehicle::new(self.entity)
    }
}

impl Deref for AbstractMinecart<'_> {
    type Target = dyn Entity;

    fn deref(&self) -> &Self::Target {
        self.entity
    }
}

/// A concrete vanilla `AbstractMinecart` type.
pub trait Parts: vehicle_entity::Parts + Has<AbstractMinecartBase> + DowncastType {
    /// Returns this entity as a minecart.
    fn abstract_minecart(&self) -> AbstractMinecart<'_>;
}

impl<E: vehicle_entity::Parts + Has<AbstractMinecartBase> + DowncastType> Parts for E {
    fn abstract_minecart(&self) -> AbstractMinecart<'_> {
        AbstractMinecart::new(self)
    }
}

/// The `AbstractMinecart.tick` stage, which replaces `Entity`'s.
pub const TICK: StageId = StageId("abstract_minecart.tick");
/// The `AbstractMinecart.getBlockSpeedFactor` stage: full speed on rails,
/// before `Entity`'s.
pub const BLOCK_SPEED_FACTOR: StageId = StageId("abstract_minecart.block_speed_factor");

/// Vanilla `AbstractMinecart`'s chains, on `VehicleEntity`'s.
#[must_use]
pub fn definition<E: Parts>() -> Definition<E> {
    fn tick<E: Parts>(minecart: &E) {
        minecart.abstract_minecart().tick();
    }
    fn block_speed_factor<E: Parts>(minecart: &E, ctx: &mut BlockSpeedFactorCtx<'_>) -> Flow {
        let Some(world) = minecart.level() else {
            return Flow::Continue;
        };
        let state = world.get_block_state(minecart.block_position());
        if !state.get_block().has_tag(&BlockTag::RAILS) {
            return Flow::Continue;
        }
        ctx.result = 1.0;
        Flow::Stop
    }
    let definition = vehicle_entity::definition::<E>();
    Definition {
        tick: Pipeline::new([Stage::effect(TICK, tick::<E>)]),
        block_speed_factor: definition.block_speed_factor.before(
            entity_definition::BLOCK_SPEED_FACTOR,
            [Stage::new(BLOCK_SPEED_FACTOR, block_speed_factor::<E>)],
        ),
        ..definition
    }
}

/// `AbstractMinecart`'s overrides of the entity hooks.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        blocks_building: |_| true,
        movement_emission: |_| EntityMovementEmission::Events,
        can_collide_with: |minecart, other| {
            (other.can_be_collided_with(Some(minecart)) || other.is_pushable())
                && !minecart.is_passenger_of_same_vehicle(other)
        },
        is_pushable: |_| true,
        is_pickable: |minecart| !minecart.is_removed(),
        relative_portal_position: |minecart, axis, portal_area| {
            relative_portal_position(minecart, axis, portal_area)
        },
        passenger_attachment_point: |minecart, passenger| {
            let passenger_type = passenger.entity_type();
            if passenger_type == &vanilla_entities::VILLAGER
                || passenger_type == &vanilla_entities::WANDERING_TRADER
            {
                LOWERED_PASSENGER_ATTACHMENT
            } else {
                minecart.passenger_attachment_point_default(passenger)
            }
        },
        motion_direction: |minecart| {
            MinecartBehavior::motion_direction(&minecart.abstract_minecart())
        },
        default_gravity: |minecart| {
            if minecart.is_in_water() {
                GRAVITY_IN_WATER
            } else {
                GRAVITY
            }
        },
        known_movement: |minecart| {
            MinecartBehavior::known_movement(minecart.known_movement_default())
        },
        air_drag: |_| AIR_DRAG,
        move_entity: |minecart, mover_type, delta| {
            let result = minecart.move_entity_default(mover_type, delta);
            minecart.abstract_minecart().apply_effects_from_blocks();
            result
        },
        is_on_rails: |minecart| minecart.abstract_minecart().on_rails(),
        dismount_location_for_passenger: |minecart, passenger| {
            minecart
                .abstract_minecart()
                .dismount_location_for_passenger(passenger)
        },
        push_entity: |minecart, other| minecart.abstract_minecart().push_entity(other),
        ..vehicle_entity::entity_props()
    }
}

/// Vanilla `AbstractMinecart.getRelativePortalPosition`: the entity's portal
/// position with the forward offset reset.
pub(super) fn relative_portal_position(
    minecart: &dyn Entity,
    axis: Axis,
    portal_area: FoundRectangle,
) -> DVec3 {
    reset_forward_direction_of_relative_portal_position(
        minecart.relative_portal_position_default(axis, portal_area),
    )
}

impl AbstractMinecart<'_> {
    fn with_synced<R>(&self, f: impl FnOnce(&mut AbstractMinecartEntityData) -> R) -> Option<R> {
        let synced = self.entity.synced_data()?;
        let mut data = synced.lock_data();
        data.abstract_minecart_layer_mut().map(f)
    }

    /// Whether the minecart stood on rails at its last tick.
    #[must_use]
    pub fn on_rails(&self) -> bool {
        self.base.state.lock().on_rails
    }

    pub(super) fn set_on_rails(&self, on_rails: bool) {
        self.base.state.lock().on_rails = on_rails;
    }

    /// Whether the movement-facing rotation is flipped.
    #[must_use]
    pub fn is_flipped(&self) -> bool {
        self.base.state.lock().flipped
    }

    /// Sets whether the movement-facing rotation is flipped.
    pub fn set_flipped(&self, flipped: bool) {
        self.base.state.lock().flipped = flipped;
    }

    pub(super) fn toggle_flipped(&self) {
        let mut state = self.base.state.lock();
        state.flipped = !state.flipped;
    }

    /// The block shown in the minecart, if one replaces the default.
    #[must_use]
    pub fn custom_display_block(&self) -> Option<BlockStateId> {
        self.with_synced(|data| *data.id_custom_display_block.get())
            .flatten()
    }

    /// Replaces the block shown in the minecart.
    pub fn set_custom_display_block(&self, state: Option<BlockStateId>) {
        self.with_synced(|data| data.id_custom_display_block.set(state));
    }

    /// The display block's vertical offset, in sixteenths of a block.
    #[must_use]
    pub fn display_offset(&self) -> i32 {
        self.with_synced(|data| *data.id_display_offset.get())
            .unwrap_or_else(|| self.default_display_offset())
    }

    /// Sets the display block's vertical offset.
    pub fn set_display_offset(&self, offset: i32) {
        self.with_synced(|data| data.id_display_offset.set(offset));
    }

    /// The block shown in the minecart.
    #[must_use]
    pub fn display_block_state(&self) -> BlockStateId {
        self.custom_display_block()
            .unwrap_or_else(|| self.default_display_block_state())
    }

    #[expect(
        clippy::unused_self,
        reason = "the hook default for every minecart type"
    )]
    const fn is_rideable_default(&self) -> bool {
        false
    }

    #[expect(
        clippy::unused_self,
        reason = "the hook default for every minecart type"
    )]
    const fn is_furnace_default(&self) -> bool {
        false
    }

    #[expect(
        clippy::unused_self,
        reason = "the hook default for every minecart type"
    )]
    const fn activate_minecart_default(&self, _world: &World, _pos: BlockPos, _powered: bool) {}

    #[expect(
        clippy::unused_self,
        reason = "the hook default for every minecart type"
    )]
    fn default_display_block_state_default(&self) -> BlockStateId {
        vanilla_blocks::AIR.default_state()
    }

    #[expect(
        clippy::unused_self,
        reason = "the hook default for every minecart type"
    )]
    const fn default_display_offset_default(&self) -> i32 {
        6
    }

    fn max_speed_default(&self, world: &World) -> f64 {
        MinecartBehavior::max_speed(self, world)
    }

    /// The rail block below the minecart, or its own block position.
    #[must_use]
    pub fn current_block_pos_or_rail_below(&self, world: &World) -> BlockPos {
        let position = self.position();
        let pos = BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        );
        let below = pos.below();
        if world
            .get_block_state(below)
            .get_block()
            .has_tag(&BlockTag::RAILS)
        {
            below
        } else {
            pos
        }
    }

    /// `AbstractMinecart.applyEffectsFromBlocks`: block contacts at the
    /// current position only, clearing the tick's movement.
    pub fn apply_effects_from_blocks(&self) {
        let position = self.position();
        self.apply_effects_from_blocks_between(position, position);
        self.base().clear_movement_this_tick();
    }

    /// `AbstractMinecart.getDismountLocationForPassenger`: a floor beside the
    /// minecart, else on top of it in the first pose that fits under the
    /// ceiling.
    #[must_use]
    pub fn dismount_location_for_passenger(&self, passenger: &Living<'_>) -> DVec3 {
        let forward = self.motion_direction();
        let Some(world) = self.level() else {
            return self.dismount_location_for_passenger_default(passenger);
        };
        if forward.axis() == Axis::Y {
            return self.dismount_location_for_passenger_default(passenger);
        }

        let vehicle_pos = self.block_position();
        let offsets = dismount_helper::offsets_for_direction(forward);
        for &pose in passenger.dismount_poses() {
            let dimensions = passenger.dimensions_for_pose(pose);
            let reach = f64::from(dimensions.width.min(MAX_DISMOUNT_REACH) / 2.0);
            for &offset_y in pose_dismount_heights(pose) {
                for (offset_x, offset_z) in offsets {
                    let target = vehicle_pos.offset(offset_x, offset_y, offset_z);
                    let floor = dismount_helper::block_floor_height(
                        dismount_helper::non_climbable_shape(&world, target),
                        dismount_helper::non_climbable_shape(&world, target.below()),
                    );
                    if !dismount_helper::is_block_floor_valid(floor) {
                        continue;
                    }
                    let location = dismount_helper::up_from_bottom_center_of(target, floor);
                    let collision_box = WorldAabb::entity_box(
                        location.x,
                        location.y,
                        location.z,
                        reach,
                        f64::from(dimensions.height),
                    );
                    if dismount_helper::can_passenger_dismount_to(
                        &world,
                        passenger.entity(),
                        &collision_box,
                    ) {
                        passenger.set_pose(pose);
                        return location;
                    }
                }
            }
        }

        let vehicle_top = self.bounding_box().max_y();
        let ceiling_from =
            BlockPos::new(vehicle_pos.x(), vehicle_top.floor() as i32, vehicle_pos.z());
        for &pose in passenger.dismount_poses() {
            let pose_height = f64::from(passenger.dimensions_for_pose(pose).height);
            let block_coverage = (vehicle_top - f64::from(ceiling_from.y()) + pose_height).ceil();
            let ceiling =
                dismount_helper::find_ceiling_from(ceiling_from, block_coverage as i32, |pos| {
                    dismount_helper::collision_shape(&world, pos)
                });
            if vehicle_top + pose_height <= ceiling {
                passenger.set_pose(pose);
                break;
            }
        }

        self.dismount_location_for_passenger_default(passenger)
    }

    /// `AbstractMinecart.tick`, which never calls `Entity`'s.
    pub fn tick(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let vehicle = self.vehicle_entity();
        if vehicle.hurt_time() > 0 {
            vehicle.set_hurt_time(vehicle.hurt_time() - 1);
        }
        if vehicle.damage() > 0.0 {
            vehicle.set_damage(vehicle.damage() - 1.0);
        }

        self.check_below_world();
        self.base().compute_known_speed();
        self.handle_portal();
        MinecartBehavior::tick(self, &world);
        self.update_fluid_interaction();

        if self.is_in_lava() {
            self.lava_ignite();
            self.lava_hurt();
            self.set_fall_distance(self.fall_distance() * LAVA_FALL_DISTANCE_FACTOR);
        }

        self.set_first_tick(false);
    }

    /// Slows horizontal movement and drops vertical movement.
    #[must_use]
    pub fn apply_natural_slowdown(&self, movement: DVec3) -> DVec3 {
        let slowdown = MinecartBehavior::slowdown_factor(self);
        let mut result = movement * DVec3::new(slowdown, 0.0, slowdown);
        if self.is_in_water() {
            result *= f64::from(WATER_SLOWDOWN_FACTOR);
        }
        result
    }

    /// Moves the minecart freely after it leaves the track.
    pub fn come_off_track(&self, world: &World) {
        let max_speed = self.max_speed(world);
        let velocity = self.velocity();
        let mut velocity = DVec3::new(
            velocity.x.clamp(-max_speed, max_speed),
            velocity.y,
            velocity.z.clamp(-max_speed, max_speed),
        );
        if self.on_ground() {
            velocity *= OFF_TRACK_GROUND_SLOWDOWN_FACTOR;
        }
        self.set_velocity(velocity);
        if self
            .move_entity(MoverType::SelfMovement, self.velocity())
            .is_none()
        {
            return;
        }

        if !self.on_ground() {
            self.set_velocity(self.velocity() * f64::from(self.air_drag()));
        }
    }

    /// `AbstractMinecart.push(Entity)`: pushes this minecart and `other`
    /// apart, exchanging velocity with another minecart.
    #[expect(
        clippy::neg_cmp_op_on_partial_ord,
        reason = "negated comparisons preserve vanilla's NaN branch behavior"
    )]
    pub fn push_entity(&self, other: &dyn Entity) {
        if other.no_physics() || self.no_physics() || self.has_passenger(other) {
            return;
        }

        let mut xa = other.position().x - self.position().x;
        let mut za = other.position().z - self.position().z;
        let dd = xa * xa + za * za;
        if !(dd >= f64::from(MIN_PUSH_DISTANCE_SQUARED)) {
            return;
        }

        let dd = dd.sqrt();
        xa /= dd;
        za /= dd;
        let pow = (1.0 / dd).min(1.0);
        xa *= pow;
        za *= pow;
        xa *= f64::from(PUSH_FORCE);
        za *= f64::from(PUSH_FORCE);
        xa *= COLLISION_PUSH_SCALE;
        za *= COLLISION_PUSH_SCALE;

        if let Some(other_minecart) = AbstractMinecart::of(other) {
            self.push_other_minecart(&other_minecart, xa, za);
        } else {
            self.push_impulse(DVec3::new(-xa, 0.0, -za));
            other.push_impulse(DVec3::new(
                xa / NON_MINECART_PUSH_DIVISOR,
                0.0,
                za / NON_MINECART_PUSH_DIVISOR,
            ));
        }
    }

    /// Exchanges horizontal velocity with a minecart ahead or behind; furnace
    /// minecarts keep most of theirs.
    #[expect(
        clippy::manual_midpoint,
        reason = "midpoint would change overflow vs vanilla"
    )]
    fn push_other_minecart(&self, other: &AbstractMinecart<'_>, xa: f64, za: f64) {
        let xo = other.position().x - self.position().x;
        let zo = other.position().z - self.position().z;

        let dir = DVec3::new(xo, 0.0, zo).normalize_or_zero();
        let yaw = f64::from(self.rotation().0.to_radians());
        let facing = DVec3::new(f64::from(trig::cos(yaw)), 0.0, f64::from(trig::sin(yaw)))
            .normalize_or_zero();
        if dir.dot(facing).abs() < f64::from(MIN_COLLISION_ALIGNMENT) {
            return;
        }

        let movement = self.velocity();
        let other_movement = other.velocity();
        let normal_scale = DVec3::new(
            COLLISION_VELOCITY_RETENTION,
            1.0,
            COLLISION_VELOCITY_RETENTION,
        );
        let furnace_scale = DVec3::new(
            FURNACE_COLLISION_VELOCITY_RETENTION,
            1.0,
            FURNACE_COLLISION_VELOCITY_RETENTION,
        );

        if other.is_furnace() && !self.is_furnace() {
            self.set_velocity(movement * normal_scale);
            self.push_impulse(DVec3::new(
                other_movement.x - xa,
                0.0,
                other_movement.z - za,
            ));
            other.set_velocity(other_movement * furnace_scale);
        } else if !other.is_furnace() && self.is_furnace() {
            other.set_velocity(other_movement * normal_scale);
            other.push_impulse(DVec3::new(movement.x + xa, 0.0, movement.z + za));
            self.set_velocity(movement * furnace_scale);
        } else {
            let xdd = (other_movement.x + movement.x) / 2.0;
            let zdd = (other_movement.z + movement.z) / 2.0;
            self.set_velocity(movement * normal_scale);
            self.push_impulse(DVec3::new(xdd - xa, 0.0, zdd - za));
            other.set_velocity(other_movement * normal_scale);
            other.push_impulse(DVec3::new(xdd + xa, 0.0, zdd + za));
        }
    }
}

/// The block heights tried above and below the minecart for each dismount pose.
const fn pose_dismount_heights(pose: EntityPose) -> &'static [i32] {
    match pose {
        EntityPose::Standing | EntityPose::Sneaking => &[0, 1, -1],
        EntityPose::Swimming => &[0, 1],
        _ => &[],
    }
}

/// Returns the block-relative exit offsets connected by the rail shape.
#[must_use]
pub const fn rail_exits(shape: RailShape) -> ((i32, i32, i32), (i32, i32, i32)) {
    const fn below((x, y, z): (i32, i32, i32)) -> (i32, i32, i32) {
        (x, y - 1, z)
    }

    let north = Direction::North.offset();
    let south = Direction::South.offset();
    let west = Direction::West.offset();
    let east = Direction::East.offset();
    match shape {
        RailShape::NorthSouth => (north, south),
        RailShape::EastWest => (west, east),
        RailShape::AscendingEast => (below(west), east),
        RailShape::AscendingWest => (west, below(east)),
        RailShape::AscendingNorth => (north, below(south)),
        RailShape::AscendingSouth => (below(north), south),
        RailShape::SouthEast => (south, east),
        RailShape::SouthWest => (south, west),
        RailShape::NorthWest => (north, west),
        RailShape::NorthEast => (north, east),
    }
}

/// Why [`create_minecart`] produced no minecart.
#[derive(Debug)]
pub(crate) enum CreateMinecartError {
    /// This minecart variant is not an [`AbstractMinecart`] in Steel yet.
    Unsupported,
    /// Creating the entity or applying the item's components failed.
    Spawn(EntitySpawnError),
}

/// Vanilla `AbstractMinecart.createMinecart`: creates and configures a
/// minecart without adding it to the world.
pub(crate) fn create_minecart(
    world: &Arc<World>,
    position: DVec3,
    entity_type: EntityTypeRef,
    item_stack: &ItemStack,
    player: Option<&Player>,
) -> Result<SharedEntity, CreateMinecartError> {
    let cart =
        create_entity_instance(world, entity_type, position).map_err(|error| match error {
            EntitySpawnError::MissingFactory => CreateMinecartError::Unsupported,
            error => CreateMinecartError::Spawn(error),
        })?;

    if AbstractMinecart::of(cart.as_ref()).is_none() {
        return Err(CreateMinecartError::Unsupported);
    }

    cart.base().set_position_local(position);
    cart.set_old_position_to_current();

    apply_item_stack_components(&cart, item_stack, player.is_some_and(Player::is_operator))
        .map_err(CreateMinecartError::Spawn)?;

    Ok(cart)
}

#[cfg(test)]
#[path = "tests/abstract_minecart.rs"]
mod tests;
