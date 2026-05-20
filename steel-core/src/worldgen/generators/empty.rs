use crate::chunk::chunk_access::ChunkAccess;
use crate::worldgen::generator::{ChunkGenerator, xoroshiro_worldgen_region_random};
use crate::worldgen::noise::beardifier::Beardifier;
use crate::worldgen::region::WorldGenRegion;
use steel_utils::ChunkPos;
use steel_utils::random::RandomSource;

/// A chunk generator that generates an empty world.
#[derive(Default)]
pub struct EmptyChunkGenerator;

impl EmptyChunkGenerator {
    /// Creates a new `EmptyWorld`.
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

impl ChunkGenerator for EmptyChunkGenerator {
    fn min_y(&self) -> i32 {
        0
    }

    fn gen_depth(&self) -> i32 {
        384
    }

    fn create_structures(&self, _chunk: &ChunkAccess) {}

    fn create_biomes(&self, _chunk: &ChunkAccess) {}

    fn fill_from_noise(&self, _chunk: &ChunkAccess, _beardifier: Option<&Beardifier>) {}

    fn build_surface(&self, _chunk: &ChunkAccess, _neighbor_biomes: &dyn Fn(i32, i32, i32) -> u16) {
    }

    fn apply_carvers(&self, _chunk: &ChunkAccess) {}

    fn create_worldgen_region_random(&self, world_seed: i64, center: ChunkPos) -> RandomSource {
        xoroshiro_worldgen_region_random(world_seed, center)
    }

    fn apply_biome_decorations(&self, _region: &mut WorldGenRegion<'_>) {}
}
