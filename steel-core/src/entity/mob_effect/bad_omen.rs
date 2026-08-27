//! `BadOmenMobEffect` behavior.

use super::MobEffectBehavior;
use crate::entity::LivingEntity;
use crate::world::World;

/// Mirrors vanilla `BadOmenMobEffect`.
///
// TODO: `applyEffectTick` checks the mob is a non-spectator `ServerPlayer`
// standing in a village (`ServerLevel.isVillage`) with no raid already at its
// max omen level, then starts/extends a `Raid` via `player.setRaidOmenPosition`
// and removes this effect (returns `false`) so `RaidOmenMobEffect` can pick up
// next tick. None of `isVillage`/`Raid`/`setRaidOmenPosition` exist yet, so
// this just keeps ticking harmlessly until the raid system lands.
pub struct BadOmenBehavior;

impl MobEffectBehavior for BadOmenBehavior {
    fn should_apply_effect_tick_this_tick(&self, _tick_count: i32, _amplifier: i32) -> bool {
        true
    }

    fn apply_effect_tick(&self, _world: &World, _user: &dyn LivingEntity, _amplifier: i32) -> bool {
        true
    }
}
