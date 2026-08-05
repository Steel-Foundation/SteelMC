//! World generation module.
//!
//! This module provides the integration between extracted vanilla worldgen data
//! and the world generation pipeline.

/// World-carving: runtime context + carver implementations.
pub mod carver;
#[cfg(test)]
mod chunk_stage_hashes;
pub(crate) mod feature;
pub mod generator;
pub mod region;
pub(crate) mod stages;
pub(crate) mod structure;
pub mod surface;
pub(crate) mod template;

pub use generator::context::{
    ChunkGeneratorType, EndGenerator, NetherGenerator, OverworldGenerator, WorldGenContext,
};
pub use generator::registry::{GeneratorOutput, WorldGeneratorRegistry};
pub use generator::{ChunkGenerator, EmptyChunkGenerator, FlatChunkGenerator, VanillaGenerator};
pub use region::WorldGenRegion;
pub use steel_worldgen::density_functions::overworld::OverworldColumnCache;
pub use steel_worldgen::noise::EndIslands;

/// Compatibility path for the per-chunk carving bitset.
pub mod carving_mask {
    pub use super::carver::CarvingMask;
}

/// Compatibility path for generator context types.
pub mod context {
    pub use super::generator::context::*;
}

/// Compatibility path for concrete generator implementations.
pub mod generators {
    pub use super::generator::{EmptyChunkGenerator, FlatChunkGenerator, VanillaGenerator};

    pub(crate) mod vanilla {
        pub(crate) use super::super::generator::vanilla::*;
    }
}

/// Compatibility path for generator factory types.
pub mod registry {
    pub use super::generator::registry::*;
}
