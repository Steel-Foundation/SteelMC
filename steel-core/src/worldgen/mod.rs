//! World generation module.
//!
//! This module provides the integration between extracted vanilla worldgen data
//! and the world generation pipeline.

mod climate_sampler;

pub use climate_sampler::VanillaClimateSampler;
pub use steel_registry::density_functions::OverworldColumnCache;
