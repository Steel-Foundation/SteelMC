//! Applies mob-effect consumption logic that cannot live in `steel-registry`
//! because it needs `LivingEntity`/`World`: `PotionContents`, generic
//! `Consumable.on_consume_effects`, and the small set of instantaneous mob
//! effects that bypass the tick loop entirely.

use std::sync::Arc;

use steel_registry::MobEffectInstance as RegistryMobEffectInstance;
use steel_registry::consume_effect::ConsumeEffectData;
use steel_registry::data_components::PotionContents;

use crate::behavior::{CONSUME_EFFECT_BEHAVIORS, MOB_EFFECT_BEHAVIORS};
use crate::entity::{Entity, LivingEntity, MobEffectInstance as RuntimeMobEffectInstance};
use crate::world::World;

/// Mirrors vanilla `PotionContents.applyToLivingEntity(user, durationScale)`.
pub(crate) fn apply_potion_contents(
    contents: &PotionContents,
    world: &World,
    user: &dyn LivingEntity,
    duration_scale: f32,
) {
    // Vanilla passes the drinker itself as both `source` and `owner` when it
    // is a player (`null` otherwise), attributing instantaneous damage to it.
    let damage_source_entity = user.as_player().map(Entity::id);
    for effect in contents.all_effects() {
        let behavior = MOB_EFFECT_BEHAVIORS.get_behavior(effect.effect());
        if behavior.is_instantaneous() {
            // Vanilla always passes `scale = 1.0` from this call site; only a
            // splash/lingering potion (not yet implemented) passes a
            // distance-based falloff scale, and a `source` distinct from
            // `owner`.
            behavior.apply_instantaneous(
                world,
                user,
                effect.amplifier(),
                damage_source_entity,
                damage_source_entity,
                1.0,
            );
            continue;
        }

        let scaled_duration = scale_effect_duration(effect.duration(), duration_scale);
        user.add_mob_effect(to_runtime_instance(&effect, scaled_duration));
    }
}

/// Mirrors vanilla `MobEffectInstance.withScaledDuration`: scales `duration`
/// by `scale`, leaving the infinite-duration sentinel (`-1`) and a zero
/// duration untouched, and never rounding a finite result below 1 tick.
fn scale_effect_duration(duration: i32, scale: f32) -> i32 {
    if duration == -1 || duration == 0 {
        return duration;
    }
    ((duration as f32 * scale).floor() as i32).max(1)
}

/// Builds the runtime active-effect state for one registry mob-effect
/// instance, ready to hand to `LivingEntity::add_mob_effect`.
pub(crate) const fn to_runtime_instance(
    effect: &RegistryMobEffectInstance,
    duration: i32,
) -> RuntimeMobEffectInstance {
    RuntimeMobEffectInstance::with_duration(effect.effect(), duration, effect.amplifier())
        .with_ambient(effect.ambient())
        .with_visible(effect.show_particles())
        .with_show_icon(effect.show_icon())
}

/// Applies one `ConsumeEffectData` entry from a `Consumable.on_consume_effects`
/// list, by looking up its registered behavior. Mirrors vanilla's
/// `ConsumeEffect.apply(Level, ItemStack, LivingEntity)` — see
/// [`crate::entity::consume_effect`] for why this is a lookup instead of the
/// direct polymorphic call vanilla uses.
pub(crate) fn apply_consume_effect(
    effect: &ConsumeEffectData,
    world: &Arc<World>,
    user: &dyn LivingEntity,
) {
    CONSUME_EFFECT_BEHAVIORS
        .get_behavior(effect.effect_type())
        .apply(effect, world, user);
}

#[cfg(test)]
mod tests {
    use steel_registry::data_components::PotionContents;
    use steel_registry::{
        MobEffectInstance as RegistryMobEffectInstance, init_vanilla_registry, vanilla_mob_effects,
    };
    use steel_utils::ChunkPos;

    use super::apply_potion_contents;
    use crate::entity::LivingEntity;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    /// Vanilla's `int` shift is masked to the low 5 bits (Java `<<` never
    /// throws), so an Instant Health/Instant Damage amplifier of 32 or more
    /// must not panic and must reproduce that masked value rather than the
    /// naive (and here overflowing) shift amount.
    #[test]
    fn instant_health_amplifier_at_shift_width_does_not_panic_and_wraps_like_vanilla() {
        init_vanilla_registry();
        let world = fresh_test_world("instant_health_high_amplifier");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let player = TestPlayerBuilder::new(world.clone(), "Test", 1).build();
        player.set_health(1.0);

        let contents = PotionContents::new(
            None,
            None,
            vec![RegistryMobEffectInstance::simple(
                vanilla_mob_effects::INSTANT_HEALTH,
                1,
                32,
            )],
            None,
        );

        apply_potion_contents(&contents, &world, player.as_ref(), 1.0);

        // 4 << 32 wraps to 4 << (32 % 32) == 4 << 0 == 4, matching Java.
        assert_eq!(player.get_health(), 5.0);
    }
}
