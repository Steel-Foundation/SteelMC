//! This module contains the `ChunkGenerator` trait, which is used to generate chunks.

pub mod context;
mod empty;
mod flat;
mod generation_chunk;
pub mod registry;
pub(crate) mod vanilla;

pub use empty::EmptyChunkGenerator;
pub use flat::FlatChunkGenerator;
#[cfg(feature = "benchmark-support")]
pub use generation_chunk::benchmark_support as generation_benchmark_support;
pub use generation_chunk::{CarversPhase, GenerationChunk, NoisePhase, SurfacePhase};
pub use vanilla::{SteelPostNoiseState, VanillaGenerator, VanillaPostNoiseStateType};

use enum_dispatch::enum_dispatch;
use glam::IVec3;
use steel_registry::biome::BiomeRef;
use steel_utils::random::{
    PositionalRandom as _, Random as _, RandomSource, RandomSplitter, name_hash::NameHash,
    xoroshiro::Xoroshiro,
};
use steel_utils::{BlockPos, ChunkPos};

use self::context::{ChunkGeneratorType, EndGenerator, NetherGenerator, OverworldGenerator};
use crate::chunk::Chunk;
use crate::worldgen::region::WorldGenRegion;
use crate::worldgen::structure::StructureGenerator;
use steel_worldgen::noise::Beardifier;

/// A trait for generating chunks.
#[enum_dispatch]
pub trait ChunkGenerator: Send + Sync {
    /// Returns the generator's minimum Y coordinate.
    fn min_y(&self) -> i32;

    /// Returns the generator's vertical generation depth in blocks.
    fn gen_depth(&self) -> i32;

    /// Returns the generator biome at one quart position without requiring a loaded chunk.
    fn noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> BiomeRef;

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
    fn create_structures(&self, chunk: &Chunk);

    /// Creates the biomes in a chunk.
    fn create_biomes(&self, chunk: &Chunk);

    /// Fills the chunk with noise.
    ///
    /// `beardifier` carries pre-collected structure-piece terrain adaptation. The caller
    /// (production: noise stage; tests: harness) is responsible for walking the chunk's
    /// structure references and building the beardifier — this trait stays free of any
    /// cross-chunk lookup. `None` skips the integration entirely (cheaper than passing
    /// an empty beardifier).
    fn fill_from_noise(
        &self,
        chunk: GenerationChunk<'_, NoisePhase>,
        beardifier: Option<&Beardifier>,
    );

    /// Builds the surface of the chunk.
    ///
    /// `neighbor_biomes` maps `(quart_x, quart_y, quart_z)` to a biome palette ID,
    /// reading from neighbor chunk palettes for out-of-chunk biome lookups (matching
    /// vanilla's `WorldGenRegion.getNoiseBiome`).
    fn build_surface(
        &self,
        chunk: GenerationChunk<'_, SurfacePhase>,
        neighbor_biomes: &dyn Fn(IVec3) -> u16,
    );

    /// Applies carvers to the chunk.
    fn apply_carvers(&self, chunk: GenerationChunk<'_, CarversPhase>);

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
