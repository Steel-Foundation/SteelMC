//! `HealOrHarmMobEffect` behavior (Instant Health / Instant Damage).

use steel_registry::vanilla_damage_types;

use super::MobEffectBehavior;
use crate::entity::LivingEntity;
use crate::entity::damage::DamageSource;
use crate::world::World;

/// Mirrors vanilla `HealOrHarmMobEffect`
pub struct HealOrHarmBehavior {
    /// `false` for Instant Health, `true` for Instant Damage.
    pub is_harm: bool,
}

impl MobEffectBehavior for HealOrHarmBehavior {
    fn is_instantaneous(&self) -> bool {
        true
    }

    /// Reached through `ApplyStatusEffectsConsumeEffect` (e.g. a golden
    /// apple-style item), one tick after `LivingEntity.addEffect`.
    fn apply_effect_tick(&self, world: &World, user: &dyn LivingEntity, amplifier: i32) -> bool {
        if self.is_harm == user.is_inverted_heal_and_harm() {
            // Mirrors vanilla's `Math.max(4 << amplification, 0)` clamp
            // against Java's signed-int overflow at extreme amplifiers.
            let amount = 4_i32.wrapping_shl(amplifier as u32).max(0);
            user.heal(amount as f32);
        } else {
            user.hurt(
                world,
                &DamageSource::environment(&vanilla_damage_types::MAGIC),
                6_i32.wrapping_shl(amplifier as u32) as f32,
            );
        }
        true
    }

    /// Reached by drinking a potion directly, or a splash/lingering potion
    /// once implemented. Unlike `apply_effect_tick`, the heal amount is
    /// never clamped to zero, and damage is attributed via
    /// `indirectMagic(source, owner)` when `direct_entity` is known.
    fn apply_instantaneous(
        &self,
        world: &World,
        user: &dyn LivingEntity,
        amplifier: i32,
        direct_entity: Option<i32>,
        causing_entity: Option<i32>,
        scale: f32,
    ) {
        if self.is_harm == user.is_inverted_heal_and_harm() {
            let amount = (scale * (4_i32.wrapping_shl(amplifier as u32) as f32) + 0.5) as i32;
            user.heal(amount as f32);
        } else {
            let mut source = DamageSource::environment(if direct_entity.is_some() {
                &vanilla_damage_types::INDIRECT_MAGIC
            } else {
                &vanilla_damage_types::MAGIC
            });
            if let Some(entity_id) = direct_entity {
                source = source.with_direct_entity(entity_id);
            }
            if let Some(entity_id) = causing_entity {
                source = source.with_causing_entity(entity_id);
            }
            // Vanilla truncates via a Java `(int)` cast; `as i32` on a
            // non-negative f32 truncates the same way.
            let amount = (scale * (6_i32.wrapping_shl(amplifier as u32) as f32) + 0.5) as i32;
            user.hurt(world, &source, amount as f32);
        }
    }
}
