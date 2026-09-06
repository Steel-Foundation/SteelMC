//! `RaidOmenMobEffect` behavior.

use super::MobEffectBehavior;
use crate::entity::LivingEntity;
use crate::world::World;

/// Mirrors vanilla `RaidOmenMobEffect`.
///
// TODO: `applyEffectTick` checks the mob is a non-spectator `ServerPlayer`
// with a pending raid-omen position, then calls `level.getRaids()
// .createOrExtendRaid(player, pos)` and clears the position, removing this
// effect (returns `false`).
pub struct RaidOmenBehavior;

impl MobEffectBehavior for RaidOmenBehavior {
    fn should_apply_effect_tick_this_tick(&self, tick_count: i32, _amplifier: i32) -> bool {
        tick_count == 1
    }

    fn apply_effect_tick(&self, _world: &World, _user: &dyn LivingEntity, _amplifier: i32) -> bool {
        true
    }
}
