//! This module contains the `ChunkGenerator` trait, which is used to generate chunks.

use enum_dispatch::enum_dispatch;
use steel_utils::BlockPos;

use crate::chunk::chunk_access::ChunkAccess;
use crate::worldgen::context::{
    ChunkGeneratorType, EndGenerator, NetherGenerator, OverworldGenerator,
};
use crate::worldgen::generators::{EmptyChunkGenerator, FlatChunkGenerator};

/// A trait for generating chunks.
#[enum_dispatch]
pub trait ChunkGenerator: Send + Sync {
    /// Returns the climate-selected origin used by vanilla before searching for a safe spawn chunk.
    fn initial_spawn_search_origin(&self) -> BlockPos {
        BlockPos::new(0, 0, 0)
    }

    /// Returns the generator-provided spawn height used before falling back to the surface heightmap.
    fn spawn_height(&self, min_y: i32, _height: i32) -> i32 {
        let _ = min_y;
        64
    }

    /// Creates the structures in a chunk.
    fn create_structures(&self, chunk: &ChunkAccess);

    /// Creates the biomes in a chunk.
    fn create_biomes(&self, chunk: &ChunkAccess);

    /// Fills the chunk with noise.
    fn fill_from_noise(&self, chunk: &ChunkAccess);

    /// Builds the surface of the chunk.
    ///
    /// `neighbor_biomes` maps `(quart_x, quart_y, quart_z)` to a biome palette ID,
    /// reading from neighbor chunk palettes for out-of-chunk biome lookups (matching
    /// vanilla's `WorldGenRegion.getNoiseBiome`).
    fn build_surface(&self, chunk: &ChunkAccess, neighbor_biomes: &dyn Fn(i32, i32, i32) -> u16);

    /// Applies carvers to the chunk.
    fn apply_carvers(&self, chunk: &ChunkAccess);

    /// Applies biome decorations to the chunk.
    fn apply_biome_decorations(&self, chunk: &ChunkAccess);
}
