//! Vanilla `PotionContents` behavior extension: the methods that need
//! `LivingEntity`/`World`, which can't live alongside the data in
//! `steel_registry::data_components::PotionContents`.

use steel_registry::MobEffectInstance as RegistryMobEffectInstance;
use steel_registry::data_components::PotionContents;

use crate::behavior::MOB_EFFECT_BEHAVIORS;
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
        if let Some(instantaneous) = behavior.as_instantaneous() {
            // Vanilla always passes `scale = 1.0` from this call site; only a
            // splash/lingering potion passes a distance-based falloff scale and
            // a `source` distinct from `owner`.
            instantaneous.apply_instantaneous(
                world,
                user,
                effect.amplifier(),
                damage_source_entity,
                damage_source_entity,
                1.0,
            );
            continue;
        }

        let scaled_effect = effect.with_scaled_duration(duration_scale);
        user.add_mob_effect(to_runtime_instance(
            &scaled_effect,
            scaled_effect.duration(),
        ));
    }
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

/// This function exists purely to reproduce Vanilla bug MC-276746,
/// where a splash potion's `show_icon` is silently replaced by its
/// `show_particles`; drop it if Mojang ever fixes it.
pub(crate) const fn to_runtime_instance_icon_from_visibility(
    effect: &RegistryMobEffectInstance,
    duration: i32,
) -> RuntimeMobEffectInstance {
    let visible = effect.show_particles();
    RuntimeMobEffectInstance::with_duration(effect.effect(), duration, effect.amplifier())
        .with_ambient(effect.ambient())
        .with_visible(visible)
        .with_show_icon(visible)
}

#[cfg(test)]
mod tests {
    use steel_registry::data_components::PotionContents;
    use steel_registry::{
        MobEffectInstance as RegistryMobEffectInstance, init_vanilla_registry, vanilla_mob_effects,
    };
    use steel_utils::ChunkPos;

    use super::{
        apply_potion_contents, to_runtime_instance, to_runtime_instance_icon_from_visibility,
    };
    use crate::behavior::init_behaviors;
    use crate::entity::LivingEntity;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn splash_rebuild_takes_its_icon_flag_from_visibility() {
        init_vanilla_registry();
        // Particles on, icon off — the only shape where the two differ.
        let effect = RegistryMobEffectInstance::new(
            vanilla_mob_effects::LUCK,
            100,
            0,
            false,
            true,
            false,
            None,
        );

        let drunk = to_runtime_instance(&effect, 100);
        assert!(drunk.is_visible());
        assert!(
            !drunk.show_icon(),
            "drinking preserves the source icon flag"
        );

        let splashed = to_runtime_instance_icon_from_visibility(&effect, 100);
        assert!(splashed.is_visible());
        assert!(
            splashed.show_icon(),
            "the splash rebuild must take show_icon from visible"
        );

        // Everything else stays identical between the two paths.
        assert_eq!(splashed.effect(), drunk.effect());
        assert_eq!(splashed.duration(), drunk.duration());
        assert_eq!(splashed.amplifier(), drunk.amplifier());
        assert_eq!(splashed.is_ambient(), drunk.is_ambient());
    }

    /// Vanilla's `int` shift is masked to the low 5 bits (Java `<<` never
    /// throws), so an Instant Health/Instant Damage amplifier of 32 or more
    /// must not panic and must reproduce that masked value rather than the
    /// naive (and here overflowing) shift amount.
    #[test]
    fn instant_health_amplifier_at_shift_width_does_not_panic_and_wraps_like_vanilla() {
        init_vanilla_registry();
        init_behaviors();
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
