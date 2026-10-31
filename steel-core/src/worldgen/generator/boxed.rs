use super::{ChunkGenerator, GenerationChunk, TerrainPhase};
use crate::{
    chunk::Chunk,
    worldgen::{region::WorldGenRegion, structure::StructureGenerator},
};
use glam::IVec3;
use steel_registry::biome::BiomeRef;
use steel_utils::{BlockPos, ChunkPos, random::RandomSource};
use steel_worldgen::noise::Beardifier;

impl<G: ChunkGenerator + ?Sized> ChunkGenerator for Box<G> {
    fn min_y(&self) -> i32 {
        (**self).min_y()
    }
    fn gen_depth(&self) -> i32 {
        (**self).gen_depth()
    }
    fn noise_biome(&self, x: i32, y: i32, z: i32) -> BiomeRef {
        (**self).noise_biome(x, y, z)
    }
    fn initial_spawn_search_origin(&self) -> BlockPos {
        (**self).initial_spawn_search_origin()
    }
    fn spawn_height(&self, min_y: i32, height: i32) -> i32 {
        (**self).spawn_height(min_y, height)
    }
    fn structure_generator(&self) -> Option<&StructureGenerator> {
        (**self).structure_generator()
    }
    fn create_structures(&self, chunk: &Chunk) {
        (**self).create_structures(chunk);
    }
    fn create_biomes(&self, chunk: &Chunk) {
        (**self).create_biomes(chunk);
    }
    fn build_terrain(
        &self,
        chunk: GenerationChunk<'_, TerrainPhase>,
        beardifier: Option<&Beardifier>,
        biomes: &dyn Fn(IVec3) -> u16,
    ) {
        (**self).build_terrain(chunk, beardifier, biomes);
    }
    fn fill_from_noise(
        &self,
        chunk: GenerationChunk<'_, TerrainPhase>,
        beardifier: Option<&Beardifier>,
    ) {
        (**self).fill_from_noise(chunk, beardifier);
    }
    fn build_surface(
        &self,
        chunk: GenerationChunk<'_, TerrainPhase>,
        biomes: &dyn Fn(IVec3) -> u16,
    ) {
        (**self).build_surface(chunk, biomes);
    }
    fn apply_carvers(&self, chunk: GenerationChunk<'_, TerrainPhase>) {
        (**self).apply_carvers(chunk);
    }
    fn create_worldgen_region_random(&self, seed: i64, center: ChunkPos) -> RandomSource {
        (**self).create_worldgen_region_random(seed, center)
    }
    fn apply_biome_decorations(&self, region: &mut WorldGenRegion<'_>) {
        (**self).apply_biome_decorations(region);
    }
}
