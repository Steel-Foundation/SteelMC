//! TNT minecart entity.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;

use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_damage_type_tags::DamageTypeTag;
use steel_registry::vanilla_entity_data::TntMinecartEntityData;
use steel_registry::vanilla_game_rules::TNT_EXPLODES;
use steel_registry::{items::ItemRef, sound_events, vanilla_blocks, vanilla_items};
use steel_utils::axis::Axis;
use steel_utils::block_util::FoundRectangle;
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::nbt::NbtValueInput as _;
use steel_utils::{BlockPos, BlockStateId, Direction, DowncastType, DowncastTypeKey};

use super::abstract_minecart::{
    AbstractMinecart, AbstractMinecartBase, minecart_relative_portal_position,
};
use super::minecart_behavior::MinecartBehavior;
use super::vehicle_entity::{
    VEHICLE_DIMENSION_CHANGING_DELAY, VehicleEntity, vehicle_ignores_explosion,
};
use crate::entity::entities::PrimedTntEntity;
use crate::entity::{
    DamageSource, Entity, EntityBase, EntityBaseLoad, EntityEventSource, EntityMovementEmission,
    EntitySyncedData, RemovalReason, SharedEntity,
};
use crate::physics::{MoveResult, MoverType};
use crate::world::{Explosion, ExplosionInteraction, ExplosionOptions, World};

/// Fuse value of a minecart that has not been primed.
const UNPRIMED_FUSE: i32 = -1;
const DEFAULT_EXPLOSION_POWER_BASE: f32 = 4.0;
const DEFAULT_EXPLOSION_SPEED_FACTOR: f32 = 1.0;
/// Horizontal speed squared at or above which a collision or destruction sets the cart off.
/// Vanilla compares against the float literal `0.01F`.
const MIN_EXPLOSIVE_HORIZONTAL_SPEED_SQR: f32 = 0.01;
/// Upper bound on the speed that contributes extra explosion power, in blocks per tick.
const MAX_EXPLOSION_SPEED: f64 = 5.0;
/// Scale applied to `speed * random` before the speed factor when adding explosion power.
const EXPLOSION_SPEED_POWER_SCALE: f64 = 1.5;
/// Exclusive bound of each of the two random rolls summed into a destruction fuse, in ticks.
const DESTROYED_FUSE_RANDOM_BOUND: i32 = 20;
/// Vanilla clamp applied to both saved explosion values.
const MAX_SAVED_EXPLOSION_VALUE: f32 = 128.0;
/// Fall distance, in blocks, from which landing sets the cart off.
const MIN_EXPLOSIVE_FALL_DISTANCE: f64 = 3.0;
/// Divisor turning fall distance into the speed fed to the explosion power formula.
const FALL_DISTANCE_SPEED_DIVISOR: f64 = 10.0;

/// The vanilla `ignitionSource` damage source.
///
/// Its direct entity is always this cart, so only the causing entity is kept; storing the full
/// [`DamageSource`] would make the cart own a strong reference to itself. The cause is retained
/// even after removal so the explosion keeps its attribution, as vanilla's stored source does.
#[derive(Clone)]
struct IgnitionSource {
    cause: Option<SharedEntity>,
}

struct TntMinecartState {
    fuse: i32,
    explosion_power_base: f32,
    explosion_speed_factor: f32,
    ignition_source: Option<IgnitionSource>,
}

impl TntMinecartState {
    const fn new() -> Self {
        Self {
            fuse: UNPRIMED_FUSE,
            explosion_power_base: DEFAULT_EXPLOSION_POWER_BASE,
            explosion_speed_factor: DEFAULT_EXPLOSION_SPEED_FACTOR,
            ignition_source: None,
        }
    }
}

/// A minecart carrying TNT.
#[entity_behavior(class = "MinecartTNT")]
pub struct TntMinecartEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<TntMinecartEntityData>,
    minecart_base: AbstractMinecartBase,
    state: SyncMutex<TntMinecartState>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `TntMinecartEntity`.
unsafe impl DowncastType for TntMinecartEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/tnt_minecart");
}

impl TntMinecartEntity {
    /// Creates a TNT minecart.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(TntMinecartEntityData::new()),
            minecart_base: AbstractMinecartBase::new(),
            state: SyncMutex::new(TntMinecartState::new()),
        }
    }

    /// Creates a TNT minecart from saved data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(TntMinecartEntityData::new()),
            minecart_base: AbstractMinecartBase::new(),
            state: SyncMutex::new(TntMinecartState::new()),
        }
    }

    /// Returns the remaining fuse in ticks, or a negative value when unprimed.
    #[must_use]
    pub fn fuse(&self) -> i32 {
        self.state.lock().fuse
    }

    /// Returns whether the fuse is lit.
    #[must_use]
    pub fn is_primed(&self) -> bool {
        self.fuse() > UNPRIMED_FUSE
    }

    /// Lights the fuse, crediting `source`'s causing entity for the eventual explosion.
    pub fn prime_fuse(&self, source: Option<&DamageSource>) {
        let Some(world) = self.level() else {
            return;
        };
        if !world.get_game_rule(&TNT_EXPLODES) {
            return;
        }

        {
            let mut state = self.state.lock();
            state.fuse = PrimedTntEntity::DEFAULT_FUSE_TIME;
            if let Some(source) = source
                && state.ignition_source.is_none()
            {
                state.ignition_source = Some(IgnitionSource {
                    cause: source.causing_entity().map(Arc::clone),
                });
            }
        }

        self.broadcast_entity_event(EntityStatus::TntPrime);
        if !self.is_silent() {
            world.play_sound_at(
                &sound_events::ENTITY_TNT_PRIMED,
                SoundSource::Blocks,
                self.position(),
                1.0,
                1.0,
                None,
            );
        }
    }

    /// Rebuilds the vanilla `ignitionSource` for this cart, if it has been ignited by a source.
    fn ignition_damage_source(&self, this: &SharedEntity) -> Option<DamageSource> {
        let ignition = self.state.lock().ignition_source.clone()?;
        Some(DamageSource::explosion(
            Some(Arc::clone(this)),
            ignition.cause,
        ))
    }

    /// Explodes with power scaled by `speed_sqr`, or just discards a primed cart while
    /// `tntExplodes` is off.
    fn explode(&self, this: &SharedEntity, damage_source: Option<DamageSource>, speed_sqr: f64) {
        let Some(world) = self.level() else {
            return;
        };

        if world.get_game_rule(&TNT_EXPLODES) {
            let (power_base, speed_factor) = {
                let state = self.state.lock();
                (state.explosion_power_base, state.explosion_speed_factor)
            };
            let speed = speed_sqr.sqrt().min(MAX_EXPLOSION_SPEED);
            let power = f64::from(power_base)
                + f64::from(speed_factor)
                    * rand::random::<f64>()
                    * EXPLOSION_SPEED_POWER_SCALE
                    * speed;

            let mut options =
                ExplosionOptions::new(self.position(), power as f32, ExplosionInteraction::Tnt);
            options.source = Some(Arc::clone(this));
            options.damage_source = damage_source;
            world.explode(options);
            self.set_removed(RemovalReason::Discarded);
        } else if self.is_primed() {
            self.set_removed(RemovalReason::Discarded);
        }
    }

    fn damage_source_ignites_tnt(source: &DamageSource) -> bool {
        if let Some(direct) = source.direct_entity()
            && direct.as_projectile().is_some()
        {
            return direct.is_on_fire();
        }
        source.is(&DamageTypeTag::IS_FIRE) || source.is(&DamageTypeTag::IS_EXPLOSION)
    }

    fn horizontal_speed_sqr(&self) -> f64 {
        let velocity = self.velocity();
        velocity.x * velocity.x + velocity.z * velocity.z
    }

    fn is_explosive_horizontal_speed(speed_sqr: f64) -> bool {
        speed_sqr >= f64::from(MIN_EXPLOSIVE_HORIZONTAL_SPEED_SQR)
    }

    /// Primed carts let explosions pass through and spare rails and the block holding them.
    fn protects_rail_at(&self, world: &World, pos: BlockPos, state: BlockStateId) -> bool {
        self.is_primed()
            && (state.get_block().has_tag(&BlockTag::RAILS)
                || world
                    .get_block_state(pos.above())
                    .get_block()
                    .has_tag(&BlockTag::RAILS))
    }
}

impl VehicleEntity for TntMinecartEntity {
    fn hurt_time(&self) -> i32 {
        *self.entity_data.lock().vehicle_entity().id_hurt.get()
    }

    fn set_hurt_time(&self, value: i32) {
        self.entity_data
            .lock()
            .vehicle_entity_mut()
            .id_hurt
            .set(value);
    }

    fn hurt_dir(&self) -> i32 {
        *self.entity_data.lock().vehicle_entity().id_hurtdir.get()
    }

    fn set_hurt_dir(&self, value: i32) {
        self.entity_data
            .lock()
            .vehicle_entity_mut()
            .id_hurtdir
            .set(value);
    }

    fn damage(&self) -> f32 {
        *self.entity_data.lock().vehicle_entity().id_damage.get()
    }

    fn set_damage(&self, value: f32) {
        self.entity_data
            .lock()
            .vehicle_entity_mut()
            .id_damage
            .set(value);
    }

    fn vehicle_drop_item(&self) -> ItemRef {
        &vanilla_items::TNT_MINECART
    }

    fn should_source_destroy(&self, source: &DamageSource) -> bool {
        Self::damage_source_ignites_tnt(source)
    }

    fn vehicle_destroy(&self, world: &World, source: &DamageSource) {
        if !Self::damage_source_ignites_tnt(source)
            && !Self::is_explosive_horizontal_speed(self.horizontal_speed_sqr())
        {
            self.vehicle_destroy_with_item(world, self.vehicle_drop_item());
            return;
        }

        if self.is_primed() {
            return;
        }
        self.prime_fuse(Some(source));
        // Vanilla overwrites the fuse even when `tntExplodes` stopped priming.
        self.state.lock().fuse = rand::random_range(0..DESTROYED_FUSE_RANDOM_BOUND)
            + rand::random_range(0..DESTROYED_FUSE_RANDOM_BOUND);
    }
}

impl AbstractMinecart for TntMinecartEntity {
    fn minecart_base(&self) -> &AbstractMinecartBase {
        &self.minecart_base
    }

    fn custom_display_block(&self) -> Option<BlockStateId> {
        *self
            .entity_data
            .lock()
            .abstract_minecart()
            .id_custom_display_block
            .get()
    }

    fn set_custom_display_block(&self, value: Option<BlockStateId>) {
        self.entity_data
            .lock()
            .abstract_minecart_mut()
            .id_custom_display_block
            .set(value);
    }

    fn display_offset(&self) -> i32 {
        *self
            .entity_data
            .lock()
            .abstract_minecart()
            .id_display_offset
            .get()
    }

    fn set_display_offset(&self, value: i32) {
        self.entity_data
            .lock()
            .abstract_minecart_mut()
            .id_display_offset
            .set(value);
    }

    fn default_display_block_state(&self) -> BlockStateId {
        vanilla_blocks::TNT.default_state()
    }

    fn activate_minecart(&self, _world: &World, _pos: BlockPos, powered: bool) {
        if powered && !self.is_primed() {
            self.prime_fuse(None);
        }
    }
}

impl Entity for TntMinecartEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(self: Arc<Self>) {
        let entity = Arc::clone(&self).into_shared_entity();
        self.tick_minecart(&entity);

        // The fuse smoke particle is client-local in vanilla.
        let fuse_burnt_out = {
            let mut state = self.state.lock();
            if state.fuse > 0 {
                state.fuse -= 1;
                false
            } else {
                state.fuse == 0
            }
        };
        if fuse_burnt_out {
            self.explode(
                &entity,
                self.ignition_damage_source(&entity),
                self.horizontal_speed_sqr(),
            );
        }

        if self.horizontal_collision() {
            let speed_sqr = self.horizontal_speed_sqr();
            if Self::is_explosive_horizontal_speed(speed_sqr) {
                self.explode(&entity, self.ignition_damage_source(&entity), speed_sqr);
            }
        }
    }

    fn get_default_gravity(&self) -> f64 {
        self.minecart_default_gravity()
    }

    fn get_air_drag(&self) -> f64 {
        self.minecart_air_drag()
    }

    fn block_speed_factor(&self) -> f32 {
        self.minecart_block_speed_factor()
    }

    fn move_entity(self: Arc<Self>, mover_type: MoverType, delta: DVec3) -> Option<MoveResult> {
        self.minecart_move_entity(mover_type, delta)
    }

    fn apply_effects_from_blocks(&self) {
        self.minecart_apply_effects_from_blocks();
    }

    fn known_movement(&self) -> DVec3 {
        MinecartBehavior::known_movement(self.default_known_movement())
    }

    fn motion_direction(&self) -> Direction {
        MinecartBehavior::motion_direction(self)
    }

    fn is_on_rails(&self) -> bool {
        self.minecart_base().on_rails()
    }

    fn is_pickable(&self) -> bool {
        self.minecart_is_pickable()
    }

    fn is_pushable(&self) -> bool {
        self.minecart_is_pushable()
    }

    fn movement_emission(&self) -> EntityMovementEmission {
        self.minecart_movement_emission()
    }

    fn blocks_building(&self) -> bool {
        self.minecart_blocks_building()
    }

    fn dimension_changing_delay(&self) -> i32 {
        VEHICLE_DIMENSION_CHANGING_DELAY
    }

    fn get_relative_portal_position(&self, axis: Axis, area: FoundRectangle) -> DVec3 {
        minecart_relative_portal_position(self, axis, area)
    }

    fn passenger_attachment_point(&self, passenger: &dyn Entity) -> DVec3 {
        self.minecart_passenger_attachment_point(passenger)
    }

    fn can_collide_with(&self, other: &dyn Entity) -> bool {
        self.minecart_can_collide_with(other)
    }

    fn push_entity(&self, entity: &dyn Entity) {
        self.minecart_push_entity(entity);
    }

    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        // TODO: Explode immediately when the direct entity is a burning `AbstractArrow`, with
        // power from the arrow's speed, once arrows exist.
        self.vehicle_hurt(world, source, amount)
    }

    fn cause_fall_damage(
        self: Arc<Self>,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool {
        if fall_distance >= MIN_EXPLOSIVE_FALL_DISTANCE {
            let fall_speed = fall_distance / FALL_DISTANCE_SPEED_DIVISOR;
            let entity = Arc::clone(&self).into_shared_entity();
            self.explode(
                &entity,
                self.ignition_damage_source(&entity),
                fall_speed * fall_speed,
            );
        }

        if self.is_fall_damage_immune() {
            return false;
        }
        self.propagate_fall_to_passengers(fall_distance, damage_modifier, source);
        false
    }

    fn ignore_explosion(&self, explosion: &dyn Explosion) -> bool {
        vehicle_ignores_explosion(explosion)
    }

    fn block_explosion_resistance(
        &self,
        _explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        _fluid: FluidState,
        resistance: f32,
    ) -> f32 {
        if self.protects_rail_at(world, pos, state) {
            0.0
        } else {
            resistance
        }
    }

    fn should_block_explode(
        &self,
        _explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        _power: f32,
    ) -> bool {
        !self.protects_rail_at(world, pos, state)
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_abstract_minecart_data(nbt);
        let state = self.state.lock();
        nbt.insert("fuse", state.fuse);
        if state.explosion_power_base != DEFAULT_EXPLOSION_POWER_BASE {
            nbt.insert("explosion_power", state.explosion_power_base);
        }
        if state.explosion_speed_factor != DEFAULT_EXPLOSION_SPEED_FACTOR {
            nbt.insert("explosion_speed_factor", state.explosion_speed_factor);
        }
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_abstract_minecart_data(nbt);
        let mut state = self.state.lock();
        state.fuse = nbt.int_value("fuse").unwrap_or(UNPRIMED_FUSE);
        state.explosion_power_base = nbt
            .float_value("explosion_power")
            .unwrap_or(DEFAULT_EXPLOSION_POWER_BASE)
            .clamp(0.0, MAX_SAVED_EXPLOSION_VALUE);
        state.explosion_speed_factor = nbt
            .float_value("explosion_speed_factor")
            .unwrap_or(DEFAULT_EXPLOSION_SPEED_FACTOR)
            .clamp(0.0, MAX_SAVED_EXPLOSION_VALUE);
    }

    // TODO: Implement getPickResult.
}

#[cfg(test)]
#[path = "tests/tnt_minecart.rs"]
mod tests;
