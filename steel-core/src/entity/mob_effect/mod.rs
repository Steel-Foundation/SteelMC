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

    /// Mirrors vanilla `MobEffect.shouldApplyEffectTickThisTick`. Instantaneous
    /// effects mirror `InstantaneousMobEffect.shouldApplyEffectTickThisTick`
    /// (`remainingDuration >= 1`) so they still fire once when added through
    /// the generic tick loop instead of the direct `applyInstantaneousEffect`
    /// call; non-instantaneous effects keep the plain `MobEffect` default.
    fn should_apply_effect_tick_this_tick(&self, tick_count: i32, _amplifier: i32) -> bool {
        self.is_instantaneous() && tick_count >= 1
    }

    /// Mirrors vanilla `MobEffect.applyEffectTick`. Returns whether the
    /// effect remains active.
    fn apply_effect_tick(&self, _world: &World, _user: &dyn LivingEntity, _amplifier: i32) -> bool {
        true
    }

    /// Mirrors vanilla `MobEffect.applyInstantaneousEffect(level, source,
    /// owner, mob, amplification, scale)`, which by default just forwards to
    /// `applyEffectTick` — most instantaneous effects (e.g. Saturation) never
    /// override this, only the amount/attribution-scaling ones like
    /// `HealOrHarmMobEffect` do. `direct_entity` is vanilla's `source` (the
    /// entity the damage is directly attributed to, e.g. a splash-potion
    /// entity) and `causing_entity` is vanilla's `owner` (the entity that
    /// ultimately caused it, e.g. the thrower) — they differ for area-effect
    /// potions and are always the same single entity for a direct drink.
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
