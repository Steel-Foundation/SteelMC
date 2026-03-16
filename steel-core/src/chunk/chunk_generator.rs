//! This module contains the `ChunkGenerator` trait, which is used to generate chunks.

use crate::chunk::chunk_access::ChunkAccess;
use enum_dispatch::enum_dispatch;

/// A trait for generating chunks.
#[enum_dispatch]
pub trait ChunkGenerator: Send + Sync {
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
