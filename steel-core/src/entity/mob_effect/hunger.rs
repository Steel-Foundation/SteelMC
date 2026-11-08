//! `HungerMobEffect` behavior.

use super::MobEffectBehavior;
use crate::entity::Living;
use crate::world::World;

/// Hunger mob effect behavior.
pub struct HungerBehavior;

impl MobEffectBehavior for HungerBehavior {
    fn should_apply_effect_tick_this_tick(&self, _tick_count: i32, _amplifier: i32) -> bool {
        true
    }

    fn apply_effect_tick(&self, _world: &World, user: &Living<'_>, amplifier: i32) -> bool {
        if let Some(player) = user.as_player() {
            player.cause_food_exhaustion(0.005 * (amplifier + 1) as f32);
        }
        true
    }
}
