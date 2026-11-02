//! Host side of particles, sounds, potion effects, experience and speeds.
use super::{parse_block_state, players::find};
use crate::{
    PlayerId, Position,
    effects::{Effect, Particle, SoundCategory},
};
use glam::DVec3;
use std::sync::Arc;
use steel_core::{
    entity::{Entity, LivingEntity, MobEffectInstance},
    server::Server as CoreServer,
    world::World as CoreWorld,
};
use steel_protocol::packets::game::{CSound, SoundSource};
use steel_registry::{
    REGISTRY, RegistryExt,
    mob_effect::MobEffectRef,
    particle_type::{
        BlockParticleOption, ColorParticleOption, DustColorTransitionOptions, DustParticleOptions,
        ParticleData, SimpleParticleOptions,
    },
    sound_event::SoundEventHolder,
};
use steel_utils::{ArgbColor, BlockStateId, DowncastType, Identifier, RgbColor};

fn key(text: &str) -> Result<Identifier, String> {
    text.parse().map_err(|error: &str| error.to_owned())
}

const fn dvec(position: Position) -> DVec3 {
    DVec3::new(position.x, position.y, position.z)
}

#[expect(
    clippy::cast_possible_wrap,
    reason = "vanilla packs colors into signed 32-bit integers"
)]
const fn rgb(color: u32) -> RgbColor {
    RgbColor::new((color & 0x00FF_FFFF) as i32)
}

#[expect(
    clippy::cast_possible_wrap,
    reason = "vanilla packs colors into signed 32-bit integers"
)]
const fn argb(color: u32) -> ArgbColor {
    ArgbColor::new(color as i32)
}

/// Builds particle options from the fields the particle's registered type expects.
pub(crate) fn particle_data(particle: &Particle) -> Result<ParticleData, String> {
    let key = key(particle.key.as_str())?;
    let particle_type = REGISTRY
        .particle_types
        .by_key(&key)
        .ok_or_else(|| format!("unknown particle {key}"))?;
    let options = particle_type.options_type_key();
    if options == SimpleParticleOptions::TYPE_KEY {
        return Ok(ParticleData::simple(particle_type));
    }
    if options == DustParticleOptions::TYPE_KEY {
        let dust = DustParticleOptions::new(rgb(particle.color), particle.scale);
        return Ok(ParticleData::new(particle_type, dust));
    }
    if options == DustColorTransitionOptions::TYPE_KEY {
        let dust = DustColorTransitionOptions::new(
            rgb(particle.color),
            rgb(particle.to_color),
            particle.scale,
        );
        return Ok(ParticleData::new(particle_type, dust));
    }
    if options == ColorParticleOption::TYPE_KEY {
        let color = ColorParticleOption::new(argb(particle.color));
        return Ok(ParticleData::new(particle_type, color));
    }
    if options == BlockParticleOption::TYPE_KEY {
        let state = parse_block_state(particle.block.as_str())?;
        let block = BlockParticleOption::new(BlockStateId(state.id));
        return Ok(ParticleData::new(particle_type, block));
    }
    // TODO: item, vibration, trail, sculk charge, shriek, spell and power particle options.
    Err(format!(
        "particle {key} needs options this API cannot build yet"
    ))
}

pub(crate) fn spawn_particle(world: &Arc<CoreWorld>, particle: &Particle) -> Result<i32, String> {
    let data = particle_data(particle)?;
    Ok(world.send_particles_with_options(
        data,
        particle.force,
        particle.force,
        dvec(particle.position),
        particle.count.max(0),
        dvec(particle.spread),
        particle.speed,
    ))
}

const fn source(category: SoundCategory) -> SoundSource {
    match category {
        SoundCategory::Master => SoundSource::Master,
        SoundCategory::Music => SoundSource::Music,
        SoundCategory::Records => SoundSource::Records,
        SoundCategory::Weather => SoundSource::Weather,
        SoundCategory::Blocks => SoundSource::Blocks,
        SoundCategory::Hostile => SoundSource::Hostile,
        SoundCategory::Neutral => SoundSource::Neutral,
        SoundCategory::Players => SoundSource::Players,
        SoundCategory::Ambient => SoundSource::Ambient,
        SoundCategory::Voice => SoundSource::Voice,
        SoundCategory::Ui => SoundSource::Ui,
    }
}

fn sound(text: &str) -> Result<SoundEventHolder, String> {
    Ok(SoundEventHolder::Direct {
        sound_id: key(text)?,
        fixed_range: None,
    })
}

pub(crate) fn play_sound(
    world: &CoreWorld,
    name: &str,
    position: Position,
    category: SoundCategory,
    volume: f32,
    pitch: f32,
) -> Result<(), String> {
    world.play_sound_holder_at(
        &sound(name)?,
        source(category),
        dvec(position),
        volume,
        pitch,
        None,
    );
    Ok(())
}

pub(crate) fn play_sound_to(
    server: &CoreServer,
    id: PlayerId,
    name: &str,
    category: SoundCategory,
    volume: f32,
    pitch: f32,
) -> Result<(), String> {
    let holder = sound(name)?;
    let player = find(server, id)?;
    player.send_packet(CSound::new_holder(
        holder,
        source(category),
        player.position(),
        volume,
        pitch,
        rand::random(),
    ));
    Ok(())
}

fn mob_effect(text: &str) -> Result<MobEffectRef, String> {
    let key = key(text)?;
    REGISTRY
        .mob_effects
        .by_key(&key)
        .ok_or_else(|| format!("unknown effect {key}"))
}

/// Builds the core instance for an API effect, resolving its key.
pub(crate) fn mob_effect_instance(effect: &Effect) -> Result<MobEffectInstance, String> {
    Ok(MobEffectInstance::with_duration(
        mob_effect(effect.key.as_str())?,
        effect.duration,
        effect.amplifier,
    )
    .with_ambient(effect.ambient)
    .with_visible(effect.particles)
    .with_show_icon(effect.icon))
}

pub(crate) fn add_effect(
    server: &CoreServer,
    id: PlayerId,
    effect: &Effect,
) -> Result<bool, String> {
    let instance = mob_effect_instance(effect)?;
    Ok(find(server, id)?.add_mob_effect(instance))
}

pub(crate) fn remove_effect(
    server: &CoreServer,
    id: PlayerId,
    effect: &str,
) -> Result<bool, String> {
    let effect = mob_effect(effect)?;
    Ok(find(server, id)?.remove_mob_effect(effect))
}

pub(crate) fn give_experience(
    server: &CoreServer,
    id: PlayerId,
    points: i32,
) -> Result<(), String> {
    find(server, id)?.give_experience_points(points);
    Ok(())
}

pub(crate) fn give_levels(server: &CoreServer, id: PlayerId, levels: i32) -> Result<(), String> {
    find(server, id)?.give_experience_levels(levels);
    Ok(())
}

fn speed(value: f32) -> Result<f32, String> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(format!("speed {value} is outside 0..=1"))
    }
}

pub(crate) fn set_walk_speed(server: &CoreServer, id: PlayerId, value: f32) -> Result<(), String> {
    let value = speed(value)?;
    find(server, id)?.set_walking_speed(value);
    Ok(())
}

pub(crate) fn set_fly_speed(server: &CoreServer, id: PlayerId, value: f32) -> Result<(), String> {
    let value = speed(value)?;
    let player = find(server, id)?;
    player.set_flying_speed(value);
    player.send_abilities();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_origin(key: &str) -> Particle {
        Particle::new(
            key,
            Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            1,
        )
    }

    #[test]
    fn particle_options_follow_the_registered_type() {
        steel_registry::init_vanilla_registry();
        let dust = at_origin("minecraft:dust").dust(0xFF_0000, 2.0);
        let data = particle_data(&dust);
        let Ok(data) = data else {
            panic!("dust should build: {data:?}");
        };
        assert!(data.downcast_ref::<DustParticleOptions>().is_some());
        assert!(particle_data(&at_origin("minecraft:note")).is_ok());
        assert!(particle_data(&at_origin("minecraft:block").block("minecraft:stone")).is_ok());
        assert!(particle_data(&at_origin("minecraft:block")).is_err());
        assert!(particle_data(&at_origin("minecraft:not_a_particle")).is_err());
    }
}
