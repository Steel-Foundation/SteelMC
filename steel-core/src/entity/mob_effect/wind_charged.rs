//! `WindChargedMobEffect` behavior.

use super::MobEffectBehavior;

// TODO: Vanilla `WindChargedMobEffect.onMobRemoved` triggers a small,
// blockless wind-burst explosion (`AbstractWindCharge.EXPLOSION_DAMAGE_CALCULATOR`)
// at the mob's death position. Needs the same `on_mob_removed` hook as
// `OozingBehavior` — see its TODO for why that doesn't exist yet — plus
// whatever explosion API backs `Level.explode`.
/// Mirrors vanilla `WindChargedMobEffect`.
pub struct WindChargedBehavior;

impl MobEffectBehavior for WindChargedBehavior {}
