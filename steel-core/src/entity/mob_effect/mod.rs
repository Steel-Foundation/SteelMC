//! Mob-effect behaviors: one small module per vanilla `MobEffect` subtype
//! under `net/minecraft/world/effect`.

mod absorption;
mod bad_omen;
mod heal_or_harm;
mod hunger;
mod infested;
mod oozing;
mod poison;
mod raid_omen;
mod regeneration;
mod saturation;
mod weaving;
mod wind_charged;
mod wither;

pub use absorption::AbsorptionBehavior;
pub use bad_omen::BadOmenBehavior;
pub use heal_or_harm::HealOrHarmBehavior;
pub use hunger::HungerBehavior;
pub use infested::InfestedBehavior;
pub use oozing::OozingBehavior;
pub use poison::PoisonBehavior;
pub use raid_omen::RaidOmenBehavior;
pub use regeneration::RegenerationBehavior;
pub use saturation::SaturationBehavior;
pub use weaving::WeavingBehavior;
pub use wind_charged::WindChargedBehavior;
pub use wither::WitherBehavior;

use crate::entity::LivingEntity;
use crate::world::World;

/// One vanilla `MobEffect` subtype's runtime behavior. Default methods match
/// vanilla's own `MobEffect` base-class defaults, so an effect with no
/// registered behavior (most of them) behaves exactly like a bare vanilla
/// `MobEffect` instance.
pub trait MobEffectBehavior: Send + Sync {
    /// Returns the instantaneous-only half of this behavior, if any.
    fn as_instantaneous(&self) -> Option<&dyn InstantaneousMobEffect> {
        None
    }

    /// Determines whether to apply the effect tick this tick.
    fn should_apply_effect_tick_this_tick(&self, tick_count: i32, _amplifier: i32) -> bool {
        self.as_instantaneous().is_some() && tick_count >= 1
    }

    /// Applies one tick of the effect. Returns whether the effect remains
    /// active.
    fn apply_effect_tick(&self, _world: &World, _user: &dyn LivingEntity, _amplifier: i32) -> bool {
        true
    }

    /// Called when the effect starts.
    fn on_effect_started(&self, _user: &dyn LivingEntity, _amplifier: i32) {}
}

/// The instantaneous-only half of a [`MobEffectBehavior`] that also extends
/// vanilla `InstantaneousMobEffect`.
pub trait InstantaneousMobEffect: MobEffectBehavior {
    /// Applies the instantaneous effect immediately, e.g. from drinking a
    /// potion or being hit by a splash/lingering one. `direct_entity` and
    /// `causing_entity` attribute any resulting damage, and `scale` applies
    /// splash/lingering falloff; the default ignores those extras and just
    /// delegates to [`apply_effect_tick`](MobEffectBehavior::apply_effect_tick).
    fn apply_instantaneous(
        &self,
        world: &World,
        user: &dyn LivingEntity,
        amplifier: i32,
        direct_entity: Option<i32>,
        causing_entity: Option<i32>,
        scale: f32,
    ) {
        let _ = (direct_entity, causing_entity, scale);
        self.apply_effect_tick(world, user, amplifier);
    }
}
