//! `SaturationMobEffect` behavior.

use super::MobEffectBehavior;
use crate::entity::LivingEntity;
use crate::world::World;

/// Mirrors vanilla `SaturationMobEffect`. Only overrides `applyEffectTick`;
/// `applyInstantaneousEffect` falls back to the trait's default forwarding,
/// matching vanilla — `SaturationMobEffect` doesn't override it either.
pub struct SaturationBehavior;

impl MobEffectBehavior for SaturationBehavior {
    fn is_instantaneous(&self) -> bool {
        true
    }

    fn apply_effect_tick(&self, _world: &World, user: &dyn LivingEntity, amplifier: i32) -> bool {
        if let Some(player) = user.as_player() {
            player.food_data.lock().eat(amplifier + 1, 1.0);
        }
        true
    }
}
