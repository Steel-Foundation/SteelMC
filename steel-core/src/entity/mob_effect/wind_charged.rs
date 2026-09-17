//! `WindChargedMobEffect` behavior.

use super::MobEffectBehavior;

// TODO: Vanilla `WindChargedMobEffect.onMobRemoved` triggers a small,
// blockless wind-burst explosion (`AbstractWindCharge.EXPLOSION_DAMAGE_CALCULATOR`)
// at the mob's death position.
/// Mirrors vanilla `WindChargedMobEffect`.
pub struct WindChargedBehavior;

impl MobEffectBehavior for WindChargedBehavior {}
