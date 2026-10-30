//! `OozingMobEffect` behavior.

use super::MobEffectBehavior;

// TODO: Vanilla `OozingMobEffect.onMobRemoved` spawns 2-4 size-2 `Slime`
// entities around the mob when it dies (capped by the `maxEntityCramming`
// game rule minus nearby slimes already there).
/// Oozing mob effect behavior.
pub struct OozingBehavior;

impl MobEffectBehavior for OozingBehavior {}
