//! Mob-effect behaviors: one small module per vanilla `MobEffect` subtype
//! under `net/minecraft/world/effect`.

mod absorption;
mod heal_or_harm;
mod hunger;
mod poison;
mod regeneration;
mod saturation;
mod wither;

pub use absorption::AbsorptionBehavior;
pub use heal_or_harm::HealOrHarmBehavior;
pub use hunger::HungerBehavior;
pub use poison::PoisonBehavior;
pub use regeneration::RegenerationBehavior;
pub use saturation::SaturationBehavior;
pub use wither::WitherBehavior;

use crate::entity::LivingEntity;
use crate::world::World;

/// One vanilla `MobEffect` subtype's runtime behavior. Default methods match
/// vanilla's own `MobEffect` base-class defaults, so an effect with no
/// registered behavior (most of them) behaves exactly like a bare vanilla
/// `MobEffect` instance.
pub trait MobEffectBehavior: Send + Sync {
    /// Mirrors vanilla `MobEffect.isInstantaneous`.
    fn is_instantaneous(&self) -> bool {
        false
    }

    /// Mirrors vanilla `MobEffect.shouldApplyEffectTickThisTick`
    fn should_apply_effect_tick_this_tick(&self, tick_count: i32, _amplifier: i32) -> bool {
        self.is_instantaneous() && tick_count >= 1
    }

    /// Mirrors vanilla `MobEffect.applyEffectTick`. Returns whether the
    /// effect remains active.
    fn apply_effect_tick(&self, _world: &World, _user: &dyn LivingEntity, _amplifier: i32) -> bool {
        true
    }

    /// Mirrors vanilla `MobEffect.applyInstantaneousEffect(level, source,
    /// owner, mob, amplification, scale)`
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

    /// Mirrors vanilla `MobEffect.onEffectStarted`
    fn on_effect_started(&self, _user: &dyn LivingEntity, _amplifier: i32) {}
}
