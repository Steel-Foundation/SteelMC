//! `AbsorptionMobEffect` behavior.

use super::MobEffectBehavior;
use crate::entity::Living;
use crate::world::World;

/// Mirrors vanilla `AbsorptionMobEffect`: expires once absorption hearts run
/// out, rather than on a duration timer.
pub struct AbsorptionBehavior;

/// Absorption HP granted per amplifier level (vanilla `4.0F`).
pub(crate) const ABSORPTION_PER_LEVEL: f32 = 4.0;

impl MobEffectBehavior for AbsorptionBehavior {
    fn should_apply_effect_tick_this_tick(&self, _tick_count: i32, _amplifier: i32) -> bool {
        true
    }

    fn apply_effect_tick(&self, _world: &World, user: &Living<'_>, _amplifier: i32) -> bool {
        user.get_absorption_amount() > 0.0
    }

    /// Grants `4 * (1 + amplifier)` absorption hearts, never lowering an
    /// existing higher amount.
    fn on_effect_started(&self, user: &Living<'_>, amplifier: i32) {
        let amount = ABSORPTION_PER_LEVEL * (1 + amplifier) as f32;
        user.set_absorption_amount(user.get_absorption_amount().max(amount));
    }
}
