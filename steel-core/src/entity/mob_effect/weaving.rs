//! `WeavingMobEffect` behavior.

use super::MobEffectBehavior;

// TODO: Vanilla `WeavingMobEffect.onMobRemoved` scatters a handful of cobweb
// blocks around the mob's death position.
/// Weaving mob effect behavior.
pub struct WeavingBehavior;

impl MobEffectBehavior for WeavingBehavior {}
