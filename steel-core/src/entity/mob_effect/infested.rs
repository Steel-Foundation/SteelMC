//! `InfestedMobEffect` behavior.

use super::MobEffectBehavior;

// TODO: Vanilla `InfestedMobEffect.onMobHurt` has a chance, whenever the
// infested mob takes damage, to spawn a few `Silverfish` fleeing outward from
// it. Needs both a `Silverfish` entity and an `on_mob_hurt` hook on
// `MobEffectBehavior` (mirroring vanilla `MobEffect.onMobHurt(ServerLevel,
// LivingEntity, int, DamageSource, float)`), fired from the damage pipeline
// — neither exists yet.
/// Mirrors vanilla `InfestedMobEffect`.
pub struct InfestedBehavior;

impl MobEffectBehavior for InfestedBehavior {}
