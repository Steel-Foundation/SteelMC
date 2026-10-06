//! `RegenerationMobEffect` behavior.

use super::MobEffectBehavior;
use crate::entity::LivingEntity;
use crate::world::World;

const HEAL_INTERVAL: i32 = 50;

/// Mirrors vanilla `RegenerationMobEffect`.
pub struct RegenerationBehavior;

impl MobEffectBehavior for RegenerationBehavior {
    fn should_apply_effect_tick_this_tick(&self, tick_count: i32, amplifier: i32) -> bool {
        let interval = HEAL_INTERVAL.wrapping_shr(amplifier as u32);
        interval <= 0 || tick_count % interval == 0
    }

    fn apply_effect_tick(&self, _world: &World, user: &dyn LivingEntity, _amplifier: i32) -> bool {
        if user.get_health() < user.get_max_health() {
            user.heal(1.0);
        }
        true
    }
}
