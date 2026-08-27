//! `WeavingMobEffect` behavior.

use super::MobEffectBehavior;

// TODO: Vanilla `WeavingMobEffect.onMobRemoved` scatters a handful of cobweb
// blocks around the mob's death position (only if it's a player, or the
// `MOB_GRIEFING` game rule allows it for non-players). Needs the same
// `on_mob_removed` hook as `OozingBehavior` — see its TODO for why that
// doesn't exist yet — plus the block-placement scan this uses.
/// Mirrors vanilla `WeavingMobEffect`.
pub struct WeavingBehavior;

impl MobEffectBehavior for WeavingBehavior {}
