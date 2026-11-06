//! `BadOmenMobEffect` behavior.

use super::MobEffectBehavior;
use crate::entity::Living;
use crate::world::World;

/// Bad omen mob effect behavior.
// TODO: `applyEffectTick` checks the mob is a non-spectator `ServerPlayer`
// standing in a village (`ServerLevel.isVillage`) with no raid already at its
// max omen level, then starts/extends a `Raid`
pub struct BadOmenBehavior;

impl MobEffectBehavior for BadOmenBehavior {
    fn should_apply_effect_tick_this_tick(&self, _tick_count: i32, _amplifier: i32) -> bool {
        true
    }

    fn apply_effect_tick(&self, _world: &World, _user: &Living<'_>, _amplifier: i32) -> bool {
        true
    }
}
