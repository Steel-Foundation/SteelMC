//! Vanilla `MinecartTNT`: a minecart carrying TNT.

use std::sync::Arc;

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::fluid::FluidState;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_damage_type_tags::DamageTypeTag;
use steel_registry::vanilla_entity_data::TntMinecartEntityData;
use steel_registry::vanilla_game_rules::TNT_EXPLODES;
use steel_registry::{sound_events, vanilla_blocks, vanilla_items};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::CheckedMutex;
use steel_utils::nbt::NbtValueInput as _;
use steel_utils::{BlockPos, BlockStateId};

use super::abstract_minecart::{
    self, AbstractMinecartBase, AbstractMinecartProps, AbstractMinecartTable,
};
use super::vehicle_entity;
use crate::entity::entities::PrimedTntEntity;
use crate::entity::prelude::*;
use crate::entity::{
    DamageSource, EntityBase, EntityProps, EntityTable, RemovalReason, SharedEntity, entity,
};
use crate::world::{Explosion, ExplosionInteraction, ExplosionOptions, World};

/// Saved fuse value of a minecart that has not been primed. Vanilla treats any negative fuse
/// as unprimed.
const UNPRIMED_FUSE: i32 = -1;
const DEFAULT_EXPLOSION_POWER_BASE: f32 = 4.0;
const DEFAULT_EXPLOSION_SPEED_FACTOR: f32 = 1.0;
/// Horizontal velocity length squared at or above which a collision or destruction sets the cart
/// off. Vanilla compares against the float literal `0.01F`.
const MIN_EXPLOSIVE_HORIZONTAL_VELOCITY_SQR: f32 = 0.01;
/// Upper bound on the velocity length that contributes extra explosion power, in blocks per tick.
const MAX_EXPLOSION_VELOCITY: f64 = 5.0;
/// Scale applied to `velocity * random` before `explosion_speed_factor` when adding explosion power.
const EXPLOSION_VELOCITY_POWER_SCALE: f64 = 1.5;
/// Exclusive bound of each of the two random rolls summed into a destruction fuse, in ticks.
const DESTROYED_FUSE_RANDOM_BOUND: i32 = 20;
/// Vanilla clamp applied to both saved explosion values.
const MAX_SAVED_EXPLOSION_VALUE: f32 = 128.0;
/// Fall distance, in blocks, from which landing sets the cart off.
const MIN_EXPLOSIVE_FALL_DISTANCE: f64 = 3.0;
/// Divisor turning fall distance into the velocity fed to the explosion power formula.
const FALL_DISTANCE_VELOCITY_DIVISOR: f64 = 10.0;

/// The vanilla `ignitionSource` damage source.
///
/// The direct entity is this cart, supplied when the blast runs so the cart
/// never owns itself. The causing entity is kept until the cart is removed,
/// even if it leaves the world first.
#[derive(Clone)]
struct IgnitionSource {
    cause: Option<SharedEntity>,
}

struct TntMinecartState {
    /// Remaining fuse in ticks, or `None` while unprimed.
    fuse: Option<i32>,
    explosion_power_base: f32,
    explosion_speed_factor: f32,
    ignition_source: Option<IgnitionSource>,
}

impl TntMinecartState {
    const fn new() -> Self {
        Self {
            fuse: None,
            explosion_power_base: DEFAULT_EXPLOSION_POWER_BASE,
            explosion_speed_factor: DEFAULT_EXPLOSION_SPEED_FACTOR,
            ignition_source: None,
        }
    }
}

/// A minecart carrying TNT.
// TODO: implement getPickResult.
#[entity_behavior(class = "MinecartTNT")]
#[derive(Entity)]
#[entity(save = Self::save_tnt, load = Self::load_tnt)]
#[entity(key = "steel:entity/tnt_minecart", constructors)]
pub struct TntMinecartEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(synced)]
    entity_data: CheckedMutex<TntMinecartEntityData>,
    #[entity(component)]
    minecart: AbstractMinecartBase,
    state: CheckedMutex<TntMinecartState>,
}

impl vehicle_entity::Parts for TntMinecartEntity {
    fn drop_item(&self) -> ItemRef {
        &vanilla_items::TNT_MINECART
    }

    fn should_source_destroy(&self, source: &DamageSource) -> bool {
        Self::damage_source_ignites_tnt(source)
    }

    /// Fire, explosions and speed light a short fuse instead of breaking the
    /// cart into its item.
    fn destroy(&self, world: &World, source: &DamageSource) {
        if !Self::damage_source_ignites_tnt(source)
            && !Self::is_explosive_horizontal_velocity(self.horizontal_velocity_sqr())
        {
            self.vehicle_entity().destroy(world, self.drop_item());
            return;
        }

        if self.is_primed() {
            return;
        }
        self.prime_fuse(Some(source));
        // Vanilla overwrites the fuse even when `tntExplodes` stopped priming.
        self.state.lock().fuse = Some(
            rand::random_range(0..DESTROYED_FUSE_RANDOM_BOUND)
                + rand::random_range(0..DESTROYED_FUSE_RANDOM_BOUND),
        );
    }
}

impl EntityTable for TntMinecartEntity {
    // TODO: override `hurt` to explode immediately when the direct entity is a burning
    // `AbstractArrow`, with power from the arrow's velocity, once arrows exist.
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: Self::entity_tick,
        cause_fall_damage: Self::entity_cause_fall_damage,
        block_explosion_resistance: Self::entity_block_explosion_resistance,
        should_block_explode: Self::entity_should_block_explode,
        on_removal: Self::entity_on_removal,
        ..abstract_minecart::entity_props()
    };
}

impl AbstractMinecartTable for TntMinecartEntity {
    const MINECART: AbstractMinecartProps<Self> = AbstractMinecartProps {
        default_display_block_state: |_| vanilla_blocks::TNT.default_state(),
        activate_minecart: Self::activate_minecart,
        ..AbstractMinecartProps::new()
    };
}

impl TntMinecartEntity {
    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        Self {
            base,
            entity_type,
            entity_data: CheckedMutex::new(TntMinecartEntityData::new()),
            minecart: AbstractMinecartBase::new(),
            state: CheckedMutex::new(TntMinecartState::new()),
        }
    }

    /// Returns the remaining fuse in ticks, or `None` when unprimed.
    #[must_use]
    pub fn fuse(&self) -> Option<i32> {
        self.state.lock().fuse
    }

    /// Returns whether the fuse is lit.
    #[must_use]
    pub fn is_primed(&self) -> bool {
        self.fuse().is_some()
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
            state.fuse = Some(PrimedTntEntity::DEFAULT_FUSE_TIME);
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
    fn ignition_damage_source(&self) -> Option<DamageSource> {
        let ignition = self.state.lock().ignition_source.clone()?;
        let this = self.shared()?;
        Some(DamageSource::explosion(Some(this), ignition.cause))
    }

    /// Explodes with power scaled by `velocity_sqr`, or just discards a primed cart while
    /// `tntExplodes` is off.
    fn explode(&self, damage_source: Option<DamageSource>, velocity_sqr: f64) {
        let Some(world) = self.level() else {
            return;
        };
        if !world.get_game_rule(&TNT_EXPLODES) {
            if self.is_primed() {
                self.set_removed(RemovalReason::Discarded);
            }
            return;
        }
        let Some(this) = self.shared() else {
            return;
        };

        let (power_base, velocity_factor) = {
            let state = self.state.lock();
            (state.explosion_power_base, state.explosion_speed_factor)
        };
        let velocity = velocity_sqr.sqrt().min(MAX_EXPLOSION_VELOCITY);
        let power = f64::from(power_base)
            + f64::from(velocity_factor)
                * rand::random::<f64>()
                * EXPLOSION_VELOCITY_POWER_SCALE
                * velocity;

        let mut options =
            ExplosionOptions::new(self.position(), power as f32, ExplosionInteraction::Tnt);
        options.source = Some(this);
        options.damage_source = damage_source;
        world.explode(options);
        self.set_removed(RemovalReason::Discarded);
    }

    fn damage_source_ignites_tnt(source: &DamageSource) -> bool {
        if let Some(direct) = source.direct_entity()
            && direct.as_projectile().is_some()
        {
            return direct.is_on_fire();
        }
        source.is(&DamageTypeTag::IS_FIRE) || source.is(&DamageTypeTag::IS_EXPLOSION)
    }

    fn horizontal_velocity_sqr(&self) -> f64 {
        let velocity = self.velocity();
        velocity.x * velocity.x + velocity.z * velocity.z
    }

    fn is_explosive_horizontal_velocity(velocity_sqr: f64) -> bool {
        velocity_sqr >= f64::from(MIN_EXPLOSIVE_HORIZONTAL_VELOCITY_SQR)
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

    /// A powered activator rail lights the fuse.
    fn activate_minecart(&self, _world: &World, _pos: BlockPos, powered: bool) {
        if powered && !self.is_primed() {
            self.prime_fuse(None);
        }
    }

    fn entity_tick(&self) {
        // A blast during movement removes the cart and drops the stored
        // ignition source, which vanilla still credits for the blasts below.
        let ignition_before_movement = self.ignition_damage_source();
        abstract_minecart::tick(self);

        // The fuse smoke particle is client-local in vanilla.
        let fuse_burnt_out = {
            let mut state = self.state.lock();
            match &mut state.fuse {
                Some(0) => true,
                Some(fuse) => {
                    *fuse -= 1;
                    false
                }
                None => false,
            }
        };

        if !fuse_burnt_out && !self.horizontal_collision() {
            return;
        }

        let ignition_source = ignition_before_movement.or_else(|| self.ignition_damage_source());
        if fuse_burnt_out {
            self.explode(ignition_source.clone(), self.horizontal_velocity_sqr());
        }

        if self.horizontal_collision() {
            let velocity_sqr = self.horizontal_velocity_sqr();
            if Self::is_explosive_horizontal_velocity(velocity_sqr) {
                self.explode(ignition_source, velocity_sqr);
            }
        }
    }

    fn entity_cause_fall_damage(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool {
        if fall_distance >= MIN_EXPLOSIVE_FALL_DISTANCE {
            let fall_velocity = fall_distance / FALL_DISTANCE_VELOCITY_DIVISOR;
            self.explode(self.ignition_damage_source(), fall_velocity * fall_velocity);
        }

        entity::cause_fall_damage(self, fall_distance, damage_modifier, source)
    }

    fn entity_block_explosion_resistance(
        &self,
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
        resistance: f32,
    ) -> f32 {
        if self.protects_rail_at(world, pos, state) {
            0.0
        } else {
            entity::block_explosion_resistance(
                self, explosion, world, pos, state, fluid, resistance,
            )
        }
    }

    fn entity_should_block_explode(
        &self,
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        power: f32,
    ) -> bool {
        !self.protects_rail_at(world, pos, state)
            && entity::should_block_explode(self, explosion, world, pos, state, power)
    }

    /// Releases the causing entity so a removed cart keeps nothing alive.
    fn entity_on_removal(&self, _reason: RemovalReason) {
        let ignition_source = self.state.lock().ignition_source.take();
        drop(ignition_source);
    }

    fn save_tnt(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        nbt.insert("fuse", state.fuse.unwrap_or(UNPRIMED_FUSE));
        if state.explosion_power_base != DEFAULT_EXPLOSION_POWER_BASE {
            nbt.insert("explosion_power", state.explosion_power_base);
        }
        if state.explosion_speed_factor != DEFAULT_EXPLOSION_SPEED_FACTOR {
            nbt.insert("explosion_speed_factor", state.explosion_speed_factor);
        }
    }

    fn load_tnt(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let mut state = self.state.lock();
        state.fuse = nbt.int_value("fuse").filter(|fuse| *fuse >= 0);
        state.explosion_power_base = nbt
            .float_value("explosion_power")
            .unwrap_or(DEFAULT_EXPLOSION_POWER_BASE)
            .clamp(0.0, MAX_SAVED_EXPLOSION_VALUE);
        state.explosion_speed_factor = nbt
            .float_value("explosion_speed_factor")
            .unwrap_or(DEFAULT_EXPLOSION_SPEED_FACTOR)
            .clamp(0.0, MAX_SAVED_EXPLOSION_VALUE);
    }
}

#[cfg(test)]
#[path = "tests/tnt_minecart.rs"]
mod tests;
