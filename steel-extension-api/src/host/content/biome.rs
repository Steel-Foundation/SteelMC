//! Builds registry biomes from extension definitions.

use std::sync::OnceLock;

use rustc_hash::FxHashMap;

use steel_registry::{
    Registry, RegistryExt,
    biome::{
        Biome, BiomeEffects, GrassColorModifier, Music, Particle, ParticleOptions,
        TemperatureModifier, WeightedMusic,
    },
    sound_event::SoundEventRef,
};
use steel_utils::Identifier;

use crate::content::BiomeDefinition;

/// Vanilla `Musics.createGameMusic` delays, in ticks.
const MUSIC_MIN_DELAY: i32 = 12_000;
const MUSIC_MAX_DELAY: i32 = 24_000;

fn key(text: &str) -> Result<Identifier, String> {
    text.parse::<Identifier>().map_err(ToString::to_string)
}

fn sound(registry: &Registry, text: &str) -> Result<SoundEventRef, String> {
    let key = key(text)?;
    registry
        .sound_events
        .by_key(&key)
        .ok_or_else(|| format!("unknown sound event {key}"))
}

fn grass_modifier(text: &str) -> Result<GrassColorModifier, String> {
    match text {
        "none" => Ok(GrassColorModifier::None),
        "swamp" => Ok(GrassColorModifier::Swamp),
        "dark_forest" => Ok(GrassColorModifier::DarkForest),
        other => Err(format!("unknown grass color modifier {other}")),
    }
}

fn option<T>(value: &stabby::option::Option<T>) -> Option<T>
where
    T: stabby::IStable + Clone + stabby::abi::IDeterminantProvider<()>,
{
    value.clone().match_owned(Some, || None)
}

pub(crate) fn register_biome(
    registry: &mut Registry,
    definition: BiomeDefinition,
) -> Result<(), String> {
    let music = option(&definition.music)
        .map(|music| -> Result<_, String> {
            Ok(vec![WeightedMusic {
                data: Music {
                    replace_current_music: false,
                    max_delay: MUSIC_MAX_DELAY,
                    min_delay: MUSIC_MIN_DELAY,
                    sound: sound(registry, music.as_str())?,
                },
                weight: 1,
            }])
        })
        .transpose()?;
    let ambient_sound = option(&definition.ambient_sound)
        .map(|ambient| sound(registry, ambient.as_str()))
        .transpose()?;
    let particle = option(&definition.particle)
        .map(|particle| -> Result<_, String> {
            Ok(Particle {
                options: ParticleOptions {
                    particle_type: key(particle.particle.as_str())?,
                },
                probability: particle.probability,
            })
        })
        .transpose()?;
    // Extension biomes have no decorators yet: generators that use them are
    // terrain-only, so features, carvers, and spawners stay empty.
    // TODO: expose spawners and placed features once generators can decorate.
    let biome = Biome {
        key: key(definition.key.as_str())?,
        has_precipitation: definition.has_precipitation,
        temperature: definition.temperature,
        downfall: definition.downfall,
        temperature_modifier: TemperatureModifier::None,
        effects: BiomeEffects {
            fog_color: definition.fog_color,
            sky_color: definition.sky_color,
            water_color: definition.water_color,
            water_fog_color: definition.water_fog_color,
            foliage_color: option(&definition.foliage_color),
            grass_color: option(&definition.grass_color),
            dry_foliage_color: option(&definition.dry_foliage_color),
            grass_color_modifier: grass_modifier(definition.grass_color_modifier.as_str())?,
            music,
            ambient_sound,
            additions_sound: None,
            mood_sound: None,
            particle,
        },
        creature_spawn_probability: 0.0,
        spawners: FxHashMap::default(),
        spawn_costs: FxHashMap::default(),
        carvers: Vec::new(),
        features: Vec::new(),
        id: OnceLock::new(),
    };
    registry.register_client_biome(biome)?;
    Ok(())
}
