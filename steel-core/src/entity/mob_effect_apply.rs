//! Applies mob-effect consumption logic that cannot live in `steel-registry`
//! because it needs `LivingEntity`/`World`: `PotionContents`, generic
//! `Consumable.on_consume_effects`, and the small set of instantaneous mob
//! effects that bypass the tick loop entirely.

use steel_protocol::packets::game::SoundSource;
use steel_registry::consume_effect::{
    ApplyStatusEffectsConsumeEffect, ClearAllStatusEffectsConsumeEffect, ConsumeEffectData,
    PlaySoundConsumeEffect, RemoveStatusEffectsConsumeEffect, vanilla_consume_effect_types,
};
use steel_registry::data_components::PotionContents;
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
        user.heal((4_i32 << effect.amplifier()) as f32);
        return;
    }
    if effect_ref == vanilla_mob_effects::INSTANT_DAMAGE {
        user.hurt(
            world,
            &DamageSource::environment(&vanilla_damage_types::MAGIC),
            (6_i32 << effect.amplifier()) as f32,
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
pub(crate) fn apply_consume_effect(effect: &ConsumeEffectData, world: &World, user: &dyn LivingEntity) {
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
            world.play_sound_at(sound, SoundSource::Players, user.position(), 1.0, 1.0, None);
        }
    }
    // TODO: TELEPORT_RANDOMLY is out of scope for the drink-potion pass.
}
