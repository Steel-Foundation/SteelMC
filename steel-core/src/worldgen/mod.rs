//! World generation module.
//!
//! This module provides the integration between extracted vanilla worldgen data
//! and the world generation pipeline.

mod biome_source;
mod climate_sampler;
mod nether_climate_sampler;

pub use biome_source::{BiomeSource, ChunkBiomeSampler, NetherBiomeSource, OverworldBiomeSource};
pub use climate_sampler::OverworldClimateSampler;
pub use nether_climate_sampler::NetherClimateSampler;
pub use steel_registry::density_functions::OverworldColumnCache;
