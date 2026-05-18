//! World generation module.
//!
//! This module provides the integration between extracted vanilla worldgen data
//! and the world generation pipeline.

/// Biome sources and climate samplers.
pub mod biomes;
/// World-carving: runtime context + carver implementations.
pub mod carver;
/// Per-chunk bitset marking positions already visited by a carver.
pub mod carving_mask;
pub mod context;
pub mod generator;
/// Concrete chunk generator implementations.
pub mod generators;
pub mod noise;
pub mod registry;
pub(crate) mod stages;
pub(crate) mod structure;
pub mod surface;

pub use biomes::{
    BiomeSourceKind, ChunkBiomeSampler, EndBiomeSource, NetherBiomeSource, OverworldBiomeSource,
};
pub use context::{
    ChunkGeneratorType, EndGenerator, NetherGenerator, OverworldGenerator, WorldGenContext,
};
pub use generator::ChunkGenerator;
pub use generators::{EmptyChunkGenerator, FlatChunkGenerator, VanillaGenerator};
pub use registry::{GeneratorOutput, WorldGeneratorRegistry};
pub use steel_worldgen::density_functions::overworld::OverworldColumnCache;
pub use steel_worldgen::noise::EndIslands;
