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

    /// Mirrors vanilla `MobEffect.shouldApplyEffectTickThisTick`.
    fn should_apply_effect_tick_this_tick(&self, tick_count: i32, _amplifier: i32) -> bool {
        self.as_instantaneous().is_some() && tick_count >= 1
    }

    /// Mirrors vanilla `MobEffect.applyEffectTick`. Returns whether the
    /// effect remains active.
    fn apply_effect_tick(&self, _world: &World, _user: &dyn LivingEntity, _amplifier: i32) -> bool {
        true
    }

    /// Mirrors vanilla `MobEffect.onEffectStarted`.
    fn on_effect_started(&self, _user: &dyn LivingEntity, _amplifier: i32) {}
}

/// The instantaneous-only half of a [`MobEffectBehavior`] that also extends
/// vanilla `InstantaneousMobEffect`.
pub trait InstantaneousMobEffect: MobEffectBehavior {
    /// Mirrors vanilla `InstantaneousMobEffect`'s override of
    /// `applyInstantaneousEffect`.
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
