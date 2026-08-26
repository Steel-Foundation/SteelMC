//! Applies mob-effect consumption logic that cannot live in `steel-registry`
//! because it needs `LivingEntity`/`World`: `PotionContents`, generic
//! `Consumable.on_consume_effects`, and the small set of instantaneous mob
//! effects that bypass the tick loop entirely.

use std::sync::Arc;

use steel_protocol::packets::game::SoundSource;
use steel_registry::consume_effect::{
    ApplyStatusEffectsConsumeEffect, ClearAllStatusEffectsConsumeEffect, ConsumeEffectData,
    PlaySoundConsumeEffect, RemoveStatusEffectsConsumeEffect, TeleportRandomlyConsumeEffect,
    vanilla_consume_effect_types,
};
use steel_registry::data_components::PotionContents;
use steel_registry::{
    MobEffectInstance as RegistryMobEffectInstance, sound_events, vanilla_damage_types,
    vanilla_game_events, vanilla_mob_effects,
};

use crate::entity::damage::DamageSource;
use crate::entity::{LivingEntity, MobEffectInstance as RuntimeMobEffectInstance};
use crate::world::World;
use crate::world::game_event::GameEventContext;

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
    world: &Arc<World>,
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
            user.play_sound(sound, 1.0, 1.0);
        }
    } else if effect_type == &vanilla_consume_effect_types::TELEPORT_RANDOMLY {
        let Some(teleport) = effect.downcast_ref::<TeleportRandomlyConsumeEffect>() else {
            return;
        };
        teleport_randomly(*teleport, world, user);
    }
}

/// Mirrors vanilla `TeleportRandomlyConsumeEffect.apply`: tries up to 16
/// random nearby positions, delegating each attempt to
/// `LivingEntity::random_teleport` (vanilla `Entity.randomTeleport`), and
/// stops at the first one that lands.
fn teleport_randomly(
    effect: TeleportRandomlyConsumeEffect,
    world: &Arc<World>,
    user: &dyn LivingEntity,
) {
    let diameter = f64::from(effect.diameter());
    let min_y = f64::from(world.get_min_y());
    let max_y = f64::from(world.get_min_y() + world.dimension_type.logical_height - 1);

    for _ in 0..16 {
        let origin = user.position();
        let x = origin.x + (rand::random::<f64>() - 0.5) * diameter;
        let y = (origin.y + (rand::random::<f64>() - 0.5) * diameter).clamp(min_y, max_y);
        let z = origin.z + (rand::random::<f64>() - 0.5) * diameter;

        if user.is_passenger() {
            user.stop_riding();
        }

        let old_pos = user.position();
        if !user.random_teleport(world, x, y, z, true) {
            continue;
        }

        world.game_event_at(
            &vanilla_game_events::TELEPORT,
            old_pos,
            &GameEventContext::new(Some(user.as_entity_event_source()), None),
        );
        // TODO: Play `FOX_TELEPORT` on `SoundSource::Neutral` instead once Fox
        // is implemented, mirroring vanilla `TeleportRandomlyConsumeEffect.apply`.
        world.play_sound_at(
            &sound_events::ITEM_CHORUS_FRUIT_TELEPORT,
            SoundSource::Players,
            user.position(),
            1.0,
            1.0,
            None,
        );
        user.reset_fall_distance();
        user.reset_current_impulse_context();
        return;
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::consume_effect::TeleportRandomlyConsumeEffect;
    use steel_registry::data_components::PotionContents;
    use steel_registry::{
        MobEffectInstance as RegistryMobEffectInstance, init_vanilla_registry, vanilla_blocks,
        vanilla_mob_effects,
    };
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos};

    use super::{apply_potion_contents, teleport_randomly};
    use crate::entity::{Entity, LivingEntity};
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

        apply_potion_contents(&contents, &world, player.as_ref());

        // 4 << 32 wraps to 4 << (32 % 32) == 4 << 0 == 4, matching Java.
        assert_eq!(player.get_health(), 5.0);
    }

    /// With no solid ground anywhere in range, every landing attempt must
    /// fail and the player must stay exactly where they started — mirroring
    /// vanilla `Entity.randomTeleport` reverting to the original position
    /// when no candidate lands.
    #[test]
    fn teleport_randomly_leaves_the_player_in_place_with_no_valid_landing() {
        init_vanilla_registry();
        let world = fresh_test_world("teleport_randomly_no_valid_landing");
        for x in -1..=0 {
            for z in -1..=0 {
                insert_ready_full_chunk(&world, ChunkPos::new(x, z));
            }
        }
        let player = TestPlayerBuilder::new(world.clone(), "Test", 1).build();
        let origin = player.position();

        teleport_randomly(
            TeleportRandomlyConsumeEffect::default_value(),
            &world,
            player.as_ref(),
        );

        assert_eq!(player.position(), origin);
    }

    /// With solid ground everywhere in range, the player must land on top of
    /// it, within the effect's diameter of the origin. Mirrors vanilla
    /// `TeleportRandomlyConsumeEffect.apply` picking the first safe landing.
    #[test]
    fn teleport_randomly_lands_on_solid_ground_within_diameter() {
        init_vanilla_registry();
        crate::behavior::init_behaviors();
        let world = fresh_test_world("teleport_randomly_valid_landing");
        for x in -1..=0 {
            for z in -1..=0 {
                insert_ready_full_chunk(&world, ChunkPos::new(x, z));
            }
        }
        for x in -8..8 {
            for z in -8..8 {
                world.set_block(
                    BlockPos::new(x, -1, z),
                    vanilla_blocks::STONE.default_state(),
                    UpdateFlags::UPDATE_ALL,
                );
            }
        }
        let player = TestPlayerBuilder::new(world.clone(), "Test", 1).build();
        let origin = player.position();

        teleport_randomly(
            TeleportRandomlyConsumeEffect::default_value(),
            &world,
            player.as_ref(),
        );

        let landed = player.position();
        assert_ne!(landed, origin);
        assert!((landed.x - origin.x).abs() <= 8.0);
        assert!((landed.z - origin.z).abs() <= 8.0);
        // The landing loop preserves the fractional part of the candidate Y
        // (see `find_ground_y`), so a floor at block Y = -1 always lands the
        // player somewhere in [0, 1) above it.
        assert!((0.0..1.0).contains(&landed.y));
    }
}
