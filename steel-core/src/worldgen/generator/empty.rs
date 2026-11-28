use crate::chunk::Chunk;
use crate::worldgen::generator::{
    ChunkGenerator, GenerationChunk, TerrainPhase, xoroshiro_worldgen_region_random,
};
use crate::worldgen::region::WorldGenRegion;
use glam::IVec3;
use steel_registry::{biome::BiomeRef, vanilla_biomes};
use steel_utils::random::RandomSource;
use steel_utils::{BlockPos, ChunkPos};
use steel_worldgen::biomes::BiomeSourceKind;
use steel_worldgen::noise::Beardifier;

/// A chunk generator that generates an empty world.
#[derive(Default)]
pub struct EmptyChunkGenerator;

impl EmptyChunkGenerator {
    const MIN_Y: i32 = 0;
    const HEIGHT: i32 = 384;

    /// Creates a new `EmptyWorld`.
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

impl ChunkGenerator for EmptyChunkGenerator {
    fn min_y(&self) -> i32 {
        Self::MIN_Y
    }

    fn gen_depth(&self) -> i32 {
        Self::HEIGHT
    }

    fn noise_biome(&self, _quart_x: i32, _quart_y: i32, _quart_z: i32) -> BiomeRef {
        &vanilla_biomes::PLAINS
    }

    fn find_closest_biome_3d(
        &self,
        origin: BlockPos,
        search_radius: i32,
        sample_resolution_horizontal: i32,
        sample_resolution_vertical: i32,
        allowed: &dyn Fn(BiomeRef) -> bool,
    ) -> Option<(BlockPos, BiomeRef)> {
        BiomeSourceKind::fixed(&vanilla_biomes::PLAINS).find_closest_biome_3d(
            origin,
            search_radius,
            sample_resolution_horizontal,
            sample_resolution_vertical,
            Self::MIN_Y,
            Self::MIN_Y + Self::HEIGHT - 1,
            allowed,
        )
    }

    fn create_structures(&self, _chunk: &Chunk) {}

    fn create_biomes(&self, _chunk: &Chunk) {}

    fn fill_from_noise(
        &self,
        _chunk: GenerationChunk<'_, TerrainPhase>,
        _beardifier: Option<&Beardifier>,
    ) {
    }

    fn build_surface(
        &self,
        _chunk: GenerationChunk<'_, TerrainPhase>,
        _neighbor_biomes: &dyn Fn(IVec3) -> u16,
    ) {
    }

    fn apply_carvers(&self, _chunk: GenerationChunk<'_, TerrainPhase>) {}

    fn create_worldgen_region_random(&self, world_seed: i64, center: ChunkPos) -> RandomSource {
        xoroshiro_worldgen_region_random(world_seed, center)
    }

    fn apply_biome_decorations(&self, _region: &mut WorldGenRegion<'_>) {}
}
