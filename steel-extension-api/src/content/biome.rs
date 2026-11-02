//! Registered biomes, sent to clients and saved in chunks by key.

use stabby::{option::Option as ApiOption, string::String as ApiString};

/// Ambient particles drifting through a biome.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct BiomeParticle {
    /// Particle type key without options, such as `minecraft:white_ash`.
    pub particle: ApiString,
    /// Chance per client tick and nearby position, from 0 to 1.
    pub probability: f32,
}

/// A biome's climate and client appearance. Colors are 24-bit RGB values.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct BiomeDefinition {
    /// Biome key, in the owner's namespace.
    pub key: ApiString,
    /// `none`, `swamp`, or `dark_forest`.
    pub grass_color_modifier: ApiString,
    /// Background music sound key, played with vanilla's default delays.
    pub music: ApiOption<ApiString>,
    /// Looping ambient sound key.
    pub ambient_sound: ApiOption<ApiString>,
    /// Ambient particles.
    pub particle: ApiOption<BiomeParticle>,
    /// Grass color; computed from climate when absent.
    pub grass_color: ApiOption<i32>,
    /// Foliage color; computed from climate when absent.
    pub foliage_color: ApiOption<i32>,
    /// Dry foliage (leaf litter) color; computed from climate when absent.
    pub dry_foliage_color: ApiOption<i32>,
    /// Sky color.
    pub sky_color: i32,
    /// Distance fog color.
    pub fog_color: i32,
    /// Water surface color.
    pub water_color: i32,
    /// Underwater fog color.
    pub water_fog_color: i32,
    /// Temperature; above 0.15 means rain instead of snow.
    pub temperature: f32,
    /// Downfall (humidity).
    pub downfall: f32,
    /// Whether rain or snow falls.
    pub has_precipitation: bool,
}

impl BiomeDefinition {
    /// A biome with the given sky, fog, and water colors and a temperate climate.
    #[must_use]
    pub fn new(
        key: &str,
        sky_color: i32,
        fog_color: i32,
        water_color: i32,
        water_fog_color: i32,
    ) -> Self {
        Self {
            key: key.into(),
            grass_color_modifier: "none".into(),
            music: None.into(),
            ambient_sound: None.into(),
            particle: None.into(),
            grass_color: None.into(),
            foliage_color: None.into(),
            dry_foliage_color: None.into(),
            sky_color,
            fog_color,
            water_color,
            water_fog_color,
            temperature: 0.8,
            downfall: 0.4,
            has_precipitation: true,
        }
    }
}
