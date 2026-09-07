//! `HungerMobEffect` behavior.

use super::MobEffectBehavior;
use crate::entity::LivingEntity;
use crate::world::World;

/// Mirrors vanilla `HungerMobEffect`.
pub struct HungerBehavior;

/// Food exhaustion caused per amplifier level, per tick (vanilla `0.005F`).
const EXHAUSTION_PER_LEVEL: f32 = 0.005;

impl MobEffectBehavior for HungerBehavior {
    fn should_apply_effect_tick_this_tick(&self, _tick_count: i32, _amplifier: i32) -> bool {
        true
    }

    fn apply_effect_tick(&self, _world: &World, user: &dyn LivingEntity, amplifier: i32) -> bool {
        if let Some(player) = user.as_player() {
            player.cause_food_exhaustion(EXHAUSTION_PER_LEVEL * (amplifier + 1) as f32);
        }
        true
    }
}
