//! Mob-effect behaviors: one small module per vanilla `MobEffect` subtype
//! under `net/minecraft/world/effect`.
//!
//! Vanilla dispatches through plain virtual calls (`this.effect.value()
//! .applyEffectTick(...)`) — most vanilla effects are just a bare `MobEffect`
//! instance with no override at all (pure duration/amplifier/attribute-
//! modifier data), and only a handful of subclasses (`HealOrHarmMobEffect`,
//! `WitherMobEffect`, ...) override any behavior. Steel looks the right
//! implementation up by `MobEffectRef` through
//! [`MOB_EFFECT_BEHAVIORS`](crate::behavior::MOB_EFFECT_BEHAVIORS), the same
//! registry-of-trait-objects pattern used for consume effects, fluids, and
//! blocks — see [`crate::entity::consume_effect`] for why this is a lookup
//! instead of vanilla's direct polymorphic call.

mod heal_or_harm;
mod hunger;
mod poison;
mod regeneration;
mod saturation;
mod wither;

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

    /// Mirrors vanilla `MobEffect.shouldApplyEffectTickThisTick`.
    fn should_apply_effect_tick_this_tick(&self, _tick_count: i32, _amplifier: i32) -> bool {
        false
    }

    /// Mirrors vanilla `MobEffect.applyEffectTick`. Returns whether the
    /// effect remains active.
    fn apply_effect_tick(&self, _world: &World, _user: &dyn LivingEntity, _amplifier: i32) -> bool {
        true
    }

    /// Mirrors vanilla `MobEffect.applyInstantaneousEffect`, which by default
    /// just forwards to `applyEffectTick` — most instantaneous effects (e.g.
    /// Saturation) never override this, only the amount/attribution-scaling
    /// ones like `HealOrHarmMobEffect` do.
    fn apply_instantaneous(
        &self,
        world: &World,
        user: &dyn LivingEntity,
        amplifier: i32,
        damage_source_entity: Option<i32>,
        scale: f32,
    ) {
        let _ = (damage_source_entity, scale);
        self.apply_effect_tick(world, user, amplifier);
    }
}
