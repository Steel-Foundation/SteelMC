//! `InfestedMobEffect` behavior.

use super::MobEffectBehavior;

// TODO: Vanilla `InfestedMobEffect.onMobHurt` has a chance, whenever the
// infested mob takes damage, to spawn a few `Silverfish`
/// Infested mob effect behavior.
pub struct InfestedBehavior;

impl MobEffectBehavior for InfestedBehavior {}
