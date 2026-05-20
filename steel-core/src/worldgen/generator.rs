//! This module contains the `ChunkGenerator` trait, which is used to generate chunks.

use enum_dispatch::enum_dispatch;
use steel_utils::random::{
    PositionalRandom as _, Random as _, RandomSource, RandomSplitter, name_hash::NameHash,
    xoroshiro::Xoroshiro,
};
use steel_utils::{BlockPos, ChunkPos};

use crate::chunk::chunk_access::ChunkAccess;
use crate::worldgen::context::{
    ChunkGeneratorType, EndGenerator, NetherGenerator, OverworldGenerator,
};
use crate::worldgen::generators::{EmptyChunkGenerator, FlatChunkGenerator};
use crate::worldgen::noise::beardifier::Beardifier;
use crate::worldgen::region::WorldGenRegion;
use crate::worldgen::structure::StructureGenerator;

/// A trait for generating chunks.
#[enum_dispatch]
pub trait ChunkGenerator: Send + Sync {
    /// Returns the generator's minimum Y coordinate.
    fn min_y(&self) -> i32;

    /// Returns the generator's vertical generation depth in blocks.
    fn gen_depth(&self) -> i32;

    /// Returns the climate-selected origin used by vanilla before searching for a safe spawn chunk.
    fn initial_spawn_search_origin(&self) -> BlockPos {
        BlockPos::new(0, 0, 0)
    }

    /// Returns the generator-provided spawn height used before falling back to the surface heightmap.
    fn spawn_height(&self, min_y: i32, _height: i32) -> i32 {
        let _ = min_y;
        64
    }

    /// Returns the structure generator used for placement and locate queries.
    fn structure_generator(&self) -> Option<&StructureGenerator> {
        None
    }

    /// Creates the structures in a chunk.
    fn create_structures(&self, chunk: &ChunkAccess);

    /// Creates the biomes in a chunk.
    fn create_biomes(&self, chunk: &ChunkAccess);

    /// Fills the chunk with noise.
    ///
    /// `beardifier` carries pre-collected structure-piece terrain adaptation. The caller
    /// (production: noise stage; tests: harness) is responsible for walking the chunk's
    /// structure references and building the beardifier — this trait stays free of any
    /// cross-chunk lookup. `None` skips the integration entirely (cheaper than passing
    /// an empty beardifier).
    fn fill_from_noise(&self, chunk: &ChunkAccess, beardifier: Option<&Beardifier>);

    /// Builds the surface of the chunk.
    ///
    /// `neighbor_biomes` maps `(quart_x, quart_y, quart_z)` to a biome palette ID,
    /// reading from neighbor chunk palettes for out-of-chunk biome lookups (matching
    /// vanilla's `WorldGenRegion.getNoiseBiome`).
    fn build_surface(&self, chunk: &ChunkAccess, neighbor_biomes: &dyn Fn(i32, i32, i32) -> u16);

    /// Applies carvers to the chunk.
    fn apply_carvers(&self, chunk: &ChunkAccess);

    /// Creates the per-region random source exposed by vanilla `WorldGenRegion.getRandom()`.
    fn create_worldgen_region_random(&self, world_seed: i64, center: ChunkPos) -> RandomSource;

    /// Applies structure piece placement and biome feature decorations.
    fn apply_biome_decorations(&self, region: &mut WorldGenRegion<'_>);
}

pub(crate) fn worldgen_region_random_from_splitter(
    splitter: &RandomSplitter,
    center: ChunkPos,
) -> RandomSource {
    const WORLDGEN_REGION_RANDOM: NameHash = NameHash::new("minecraft:worldgen_region_random");

    let mut named_random = splitter.with_hash_of(&WORLDGEN_REGION_RANDOM);
    let region_factory = named_random.next_positional();
    region_factory.at(center.0.x * 16, 0, center.0.y * 16)
}

pub(crate) fn xoroshiro_worldgen_region_random(world_seed: i64, center: ChunkPos) -> RandomSource {
    let splitter = Xoroshiro::from_seed(world_seed as u64).next_positional();
    worldgen_region_random_from_splitter(&splitter, center)
}
