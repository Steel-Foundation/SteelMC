//! `WitherMobEffect` behavior.

use steel_registry::vanilla_damage_types;

use super::MobEffectBehavior;
use crate::entity::LivingEntity;
use crate::entity::damage::DamageSource;
use crate::world::World;

const DAMAGE_INTERVAL: i32 = 40;

/// Mirrors vanilla `WitherMobEffect`.
pub struct WitherBehavior;

impl MobEffectBehavior for WitherBehavior {
    fn should_apply_effect_tick_this_tick(&self, tick_count: i32, amplifier: i32) -> bool {
        let interval = DAMAGE_INTERVAL.wrapping_shr(amplifier as u32);
        interval <= 0 || tick_count % interval == 0
    }

    fn apply_effect_tick(&self, world: &World, user: &dyn LivingEntity, _amplifier: i32) -> bool {
        user.hurt(
            world,
            &DamageSource::environment(&vanilla_damage_types::WITHER),
            1.0,
        );
        true
    }
}
