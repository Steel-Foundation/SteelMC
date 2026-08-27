//! `OozingMobEffect` behavior.

use super::MobEffectBehavior;

// TODO: Vanilla `OozingMobEffect.onMobRemoved` spawns 2-4 size-2 `Slime`
// entities around the mob when it dies (capped by the `maxEntityCramming`
// game rule minus nearby slimes already there). Needs both a `Slime` entity
// and an `on_mob_removed` hook on `MobEffectBehavior` (mirroring vanilla
// `MobEffect.onMobRemoved(ServerLevel, LivingEntity, int,
// Entity.RemovalReason)`), fired from wherever entity removal is handled —
// neither exists yet.
/// Mirrors vanilla `OozingMobEffect`.
pub struct OozingBehavior;

impl MobEffectBehavior for OozingBehavior {}
