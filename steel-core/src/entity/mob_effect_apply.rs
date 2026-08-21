//! Applies mob-effect consumption logic that cannot live in `steel-registry`
//! because it needs `LivingEntity`/`World`: `PotionContents`, generic
//! `Consumable.on_consume_effects`, and the small set of instantaneous mob
//! effects that bypass the tick loop entirely.

use steel_registry::consume_effect::{
    ApplyStatusEffectsConsumeEffect, ClearAllStatusEffectsConsumeEffect, ConsumeEffectData,
    PlaySoundConsumeEffect, RemoveStatusEffectsConsumeEffect, vanilla_consume_effect_types,
};
use steel_registry::data_components::PotionContents;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::{
    MobEffectInstance as RegistryMobEffectInstance, vanilla_damage_types, vanilla_mob_effects,
};

use crate::entity::damage::DamageSource;
use crate::entity::{LivingEntity, MobEffectInstance as RuntimeMobEffectInstance};
use crate::world::World;

/// Mirrors vanilla `PotionContents.applyToLivingEntity(user, 1.0)`.
pub(crate) fn apply_potion_contents(
    contents: &PotionContents,
    world: &World,
    user: &dyn LivingEntity,
) {
    for effect in contents.all_effects() {
        apply_mob_effect_instance(&effect, world, user);
    }
}

/// Applies one registry mob-effect instance, dispatching the instantaneous
/// effects (Instant Health, Instant Damage, Saturation) directly instead of
/// adding them to the entity's active-effect list, mirroring vanilla
/// `MobEffect.applyInstantaneousEffect` vs. `LivingEntity.addEffect`.
fn apply_mob_effect_instance(
    effect: &RegistryMobEffectInstance,
    world: &World,
    user: &dyn LivingEntity,
) {
    let effect_ref = effect.effect();
    // TODO: Mirror vanilla `LivingEntity.isInvertedHealAndHarm()` (undead mobs
    // swap heal/harm for Instant Health/Instant Damage) once entity-type tags
    // expose that classification.
    if effect_ref == vanilla_mob_effects::INSTANT_HEALTH {
        user.heal(4_i32.wrapping_shl(effect.amplifier() as u32) as f32);
        return;
    }
    if effect_ref == vanilla_mob_effects::INSTANT_DAMAGE {
        user.hurt(
            world,
            &DamageSource::environment(&vanilla_damage_types::MAGIC),
            6_i32.wrapping_shl(effect.amplifier() as u32) as f32,
        );
        return;
    }
    if effect_ref == vanilla_mob_effects::SATURATION {
        if let Some(player) = user.as_player() {
            player.food_data.lock().eat(effect.amplifier() + 1, 1.0);
        }
        return;
    }

    let runtime =
        RuntimeMobEffectInstance::with_duration(effect_ref, effect.duration(), effect.amplifier())
            .with_ambient(effect.ambient())
            .with_visible(effect.show_particles())
            .with_show_icon(effect.show_icon());
    user.add_mob_effect(runtime);
}

/// Applies one `ConsumeEffectData` entry from a `Consumable.on_consume_effects`
/// list. Mirrors vanilla's `ConsumeEffect` subtypes in
/// `net/minecraft/world/item/consume_effects`.
pub(crate) fn apply_consume_effect(
    effect: &ConsumeEffectData,
    world: &World,
    user: &dyn LivingEntity,
) {
    let effect_type = effect.effect_type();
    if effect_type == &vanilla_consume_effect_types::APPLY_EFFECTS {
        let Some(apply) = effect.downcast_ref::<ApplyStatusEffectsConsumeEffect>() else {
            return;
        };
        if rand::random::<f32>() >= apply.probability() {
            return;
        }
        for instance in apply.effects() {
            apply_mob_effect_instance(instance, world, user);
        }
    } else if effect_type == &vanilla_consume_effect_types::REMOVE_EFFECTS {
        let Some(remove) = effect.downcast_ref::<RemoveStatusEffectsConsumeEffect>() else {
            return;
        };
        for active in user.active_mob_effects() {
            if remove.effects().contains(active.effect()) {
                user.remove_mob_effect(active.effect());
            }
        }
    } else if effect_type == &vanilla_consume_effect_types::CLEAR_ALL_EFFECTS {
        let _ = effect.downcast_ref::<ClearAllStatusEffectsConsumeEffect>();
        for active in user.active_mob_effects() {
            user.remove_mob_effect(active.effect());
        }
    } else if effect_type == &vanilla_consume_effect_types::PLAY_SOUND {
        let Some(play_sound) = effect.downcast_ref::<PlaySoundConsumeEffect>() else {
            return;
        };
        if let Some(sound) = play_sound.sound().registry_ref() {
            play_entity_sound(world, sound, user);
        }
    }
    // TODO: TELEPORT_RANDOMLY is out of scope for the drink-potion pass.
}

/// Plays a sound at `user`'s position, in `user`'s own sound category
/// (`SoundSource::Players` for players, a mob-appropriate category for
/// others). Mirrors vanilla `LivingEntity.playSound`, which always uses
/// `this.getSoundSource()` rather than a fixed category.
pub(crate) fn play_entity_sound(world: &World, sound: SoundEventRef, user: &dyn LivingEntity) {
    world.play_sound_at(sound, user.sound_source(), user.position(), 1.0, 1.0, None);
}

#[cfg(test)]
mod tests {
    use steel_registry::data_components::PotionContents;
    use steel_registry::{
        MobEffectInstance as RegistryMobEffectInstance, init_vanilla_registry, vanilla_mob_effects,
    };
    use steel_utils::ChunkPos;
    use uuid::Uuid;

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
        let player = TestPlayerBuilder::new(world.clone(), Uuid::from_u128(1), "Test", 1).build();
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

        apply_potion_contents(&contents, &world, player.as_ref());

        // 4 << 32 wraps to 4 << (32 % 32) == 4 << 0 == 4, matching Java.
        assert_eq!(player.get_health(), 5.0);
    }
}
