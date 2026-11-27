//! `PoisonMobEffect` behavior.

use steel_registry::vanilla_damage_types;

use super::MobEffectBehavior;
use crate::entity::LivingEntity;
use crate::entity::damage::DamageSource;
use crate::world::World;

const DAMAGE_INTERVAL: i32 = 25;

/// Mirrors vanilla `PoisonMobEffect`. Poison never kills: it stops dealing
/// damage once the entity's health drops to 1.0 or below.
pub struct PoisonBehavior;

impl MobEffectBehavior for PoisonBehavior {
    fn should_apply_effect_tick_this_tick(&self, tick_count: i32, amplifier: i32) -> bool {
        let interval = DAMAGE_INTERVAL.wrapping_shr(amplifier as u32);
        interval <= 0 || tick_count % interval == 0
    }

    fn apply_effect_tick(&self, world: &World, user: &dyn LivingEntity, _amplifier: i32) -> bool {
        if user.get_health() > 1.0 {
            user.hurt(
                world,
                &DamageSource::environment(&vanilla_damage_types::MAGIC),
                1.0,
            );
        }
        true
    }
}
