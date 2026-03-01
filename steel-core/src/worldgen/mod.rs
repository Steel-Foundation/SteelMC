//! World generation module.
//!
//! This module provides the integration between extracted vanilla worldgen data
//! and the world generation pipeline.

mod biome_source;
mod climate_sampler;
mod end_islands;
mod nether_climate_sampler;

pub use biome_source::{
    BiomeSourceKind, ChunkBiomeSampler, EndBiomeSource, NetherBiomeSource, OverworldBiomeSource,
};
pub use climate_sampler::OverworldClimateSampler;
pub use nether_climate_sampler::NetherClimateSampler;
pub use steel_registry::density_functions::OverworldColumnCache;
