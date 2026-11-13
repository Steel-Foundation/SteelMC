//! Vanilla `VehicleEntity`: minecarts and boats, which take damage until they
//! break into their item.
//!
//! The hurt animation and damage live in the synced `VehicleEntity` data
//! layer, reached through the [`Vehicle`] view; entity types implement
//! [`Parts`] and start their hooks from [`entity_props`].

use std::ops::Deref;

use steel_registry::data_components::vanilla_components::CUSTOM_NAME;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_entity_data::VehicleEntityData;
use steel_registry::vanilla_game_events;
use steel_registry::vanilla_game_rules::{ENTITY_DROPS, MOB_GRIEFING};

use crate::entity::prelude::*;
use crate::entity::{DamageSource, Entity, EntityProps, RemovalReason};
use crate::world::{Explosion, World};

const DAMAGE_MULTIPLIER: f32 = 10.0;
const DESTROY_DAMAGE_THRESHOLD: f32 = 40.0;
/// Ticks the hurt animation lasts.
pub(super) const HURT_DURATION: i32 = 10;
/// The portal cooldown after a vehicle changes dimension.
pub(super) const DIMENSION_CHANGING_DELAY: i32 = 10;

/// What a vehicle type defines on top of the shared layer.
pub trait Parts: Entity {
    /// The item the vehicle drops when destroyed.
    fn drop_item(&self) -> ItemRef;

    /// Whether `source` destroys the vehicle whatever its damage.
    fn should_source_destroy(&self, _source: &DamageSource) -> bool {
        false
    }

    /// Returns this entity as a vehicle.
    fn vehicle_entity(&self) -> Vehicle<'_>
    where
        Self: Sized,
    {
        Vehicle::new(self)
    }
}

/// An entity seen as a vanilla `VehicleEntity`.
#[derive(Clone, Copy)]
pub struct Vehicle<'a> {
    entity: &'a dyn Entity,
}

impl<'a> Vehicle<'a> {
    /// Views `entity`, whose synced data has the vehicle layer, as a vehicle.
    pub(super) const fn new(entity: &'a dyn Entity) -> Self {
        Self { entity }
    }

    /// Returns `entity` as a vehicle, if its synced data has the vehicle layer.
    #[must_use]
    pub fn of(entity: &'a dyn Entity) -> Option<Self> {
        entity
            .synced_data()?
            .lock_data()
            .vehicle_entity_layer()
            .map(|_| Self { entity })
    }

    fn with_synced<R>(&self, f: impl FnOnce(&mut VehicleEntityData) -> R) -> Option<R> {
        let synced = self.entity.synced_data()?;
        let mut data = synced.lock_data();
        data.vehicle_entity_layer_mut().map(f)
    }

    /// Remaining ticks of the hurt animation.
    #[must_use]
    pub fn hurt_time(&self) -> i32 {
        self.with_synced(|data| *data.id_hurt.get())
            .unwrap_or_default()
    }

    /// Sets the remaining ticks of the hurt animation.
    pub fn set_hurt_time(&self, value: i32) {
        self.with_synced(|data| data.id_hurt.set(value));
    }

    /// The direction the hurt animation rocks in.
    #[must_use]
    pub fn hurt_dir(&self) -> i32 {
        self.with_synced(|data| *data.id_hurtdir.get())
            .unwrap_or_default()
    }

    /// Sets the direction the hurt animation rocks in.
    pub fn set_hurt_dir(&self, value: i32) {
        self.with_synced(|data| data.id_hurtdir.set(value));
    }

    /// The accumulated damage, which decays every tick.
    #[must_use]
    pub fn damage(&self) -> f32 {
        self.with_synced(|data| *data.id_damage.get())
            .unwrap_or_default()
    }

    /// Sets the accumulated damage.
    pub fn set_damage(&self, value: f32) {
        self.with_synced(|data| data.id_damage.set(value));
    }

    /// Kills the vehicle and, with entity drops on, drops `drop_item`
    /// carrying the vehicle's custom name.
    pub fn destroy(&self, world: &World, drop_item: ItemRef) {
        self.kill(world);
        if !world.get_game_rule(&ENTITY_DROPS) {
            return;
        }
        let mut item = ItemStack::new(drop_item);
        if let Some(name) = self.custom_name() {
            item.set(CUSTOM_NAME, name);
        }
        self.spawn_at_location(item, 0.0);
    }
}

impl Deref for Vehicle<'_> {
    type Target = dyn Entity;

    fn deref(&self) -> &Self::Target {
        self.entity
    }
}

/// `VehicleEntity`'s [`EntityProps::dimension_changing_delay`].
#[must_use]
pub const fn dimension_changing_delay<E: Parts>(_: &E) -> i32 {
    DIMENSION_CHANGING_DELAY
}

/// `VehicleEntity`'s overrides of the entity hooks.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        hurt: hurt::<E>,
        ignore_explosion: ignore_explosion::<E>,
        dimension_changing_delay: dimension_changing_delay::<E>,
        ..EntityProps::new()
    }
}

/// `VehicleEntity`'s [`EntityProps::hurt`]: damage accumulates until the vehicle breaks.
#[expect(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "negated comparisons preserve vanilla's NaN branch behavior"
)]
pub fn hurt<E: Parts>(entity: &E, world: &World, source: &DamageSource, amount: f32) -> bool {
    if entity.is_removed() {
        return true;
    }
    if entity.is_invulnerable_to_base(source) {
        return false;
    }
    let vehicle = entity.vehicle_entity();
    vehicle.set_hurt_dir(-vehicle.hurt_dir());
    vehicle.set_hurt_time(HURT_DURATION);
    entity.mark_hurt();
    let damage = vehicle.damage() + amount * DAMAGE_MULTIPLIER;
    vehicle.set_damage(damage);

    let source_entity = source.causing_entity();
    entity.game_event_with_source_entity(
        &vanilla_game_events::ENTITY_DAMAGE,
        source_entity.map(AsRef::as_ref),
    );

    let creative_player = source_entity
        .and_then(|entity| entity.as_player())
        .is_some_and(|player| player.abilities.lock().instabuild);
    if (creative_player || !(damage > DESTROY_DAMAGE_THRESHOLD))
        && !entity.should_source_destroy(source)
    {
        if creative_player {
            entity.set_removed(RemovalReason::Discarded);
        }
    } else {
        vehicle.destroy(world, entity.drop_item());
    }
    true
}

/// Mob explosions leave vehicles intact while `mobGriefing` is off.
pub fn ignore_explosion<E: Entity>(_vehicle: &E, explosion: &dyn Explosion) -> bool {
    explosion
        .indirect_source_entity()
        .is_some_and(|entity| entity.as_mob().is_some())
        && !explosion.world().get_game_rule(&MOB_GRIEFING)
}

#[cfg(test)]
#[path = "tests/vehicle_entity.rs"]
mod tests;
