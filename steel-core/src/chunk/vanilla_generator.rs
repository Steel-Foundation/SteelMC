use steel_registry::density_functions::OverworldColumnCache;
use steel_registry::multi_noise::get_overworld_biome_cached;
use steel_registry::{REGISTRY, vanilla_blocks};

use crate::chunk::chunk_access::ChunkAccess;
use crate::chunk::chunk_generator::ChunkGenerator;
use crate::worldgen::VanillaClimateSampler;

/// A chunk generator for vanilla (normal) world generation.
///
/// Uses the multi-noise climate sampler to determine biomes at each position,
/// matching vanilla's `NoiseBasedChunkGenerator` with `MultiNoiseBiomeSource`.
pub struct VanillaGenerator {
    /// Climate sampler for biome generation (compiled density functions).
    climate_sampler: VanillaClimateSampler,
}

impl VanillaGenerator {
    /// Creates a new `VanillaGenerator` with the given seed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            climate_sampler: VanillaClimateSampler::new(seed),
        }
    }
}

impl ChunkGenerator for VanillaGenerator {
    fn create_structures(&self, _chunk: &ChunkAccess) {}

    fn create_biomes(&self, chunk: &ChunkAccess) {
        let pos = chunk.pos();
        let min_y = chunk.min_y();
        let section_count = chunk.sections().sections.len();

        let chunk_x = pos.0.x;
        let chunk_z = pos.0.y;

        // Per-chunk biome lookup cache
        let mut biome_cache: Option<usize> = None;
        // Column cache for flat-cached density function values (xz-only)
        let mut column_cache = OverworldColumnCache::new();

        // Sample biomes for each section
        for section_index in 0..section_count {
            let section_y = (min_y / 16) + section_index as i32;
            let section = &chunk.sections().sections[section_index];
            let mut section_guard = section.write();

            // For each biome position in the section (4x4x4)
            for local_quart_x in 0..4i32 {
                for local_quart_y in 0..4i32 {
                    for local_quart_z in 0..4i32 {
                        // Calculate global quart position
                        let quart_x = chunk_x * 4 + local_quart_x;
                        let quart_y = section_y * 4 + local_quart_y;
                        let quart_z = chunk_z * 4 + local_quart_z;

                        // Sample climate at this quart position
                        let target = self.climate_sampler.sample(
                            quart_x,
                            quart_y,
                            quart_z,
                            &mut column_cache,
                        );

                        // Get the biome for this climate target
                        let biome = get_overworld_biome_cached(&target, &mut biome_cache);
                        let biome_id = *REGISTRY.biomes.get_id(biome) as u16;

                        section_guard.biomes.set(
                            local_quart_x as usize,
                            local_quart_y as usize,
                            local_quart_z as usize,
                            biome_id,
                        );
                    }
                }
            }
            drop(section_guard);
        }

        chunk.mark_dirty();
    }

    fn fill_from_noise(&self, chunk: &ChunkAccess) {
        // TEMP FOR SEEING BIOMES
        for x in 0..16 {
            for z in 0..16 {
                // Bedrock at bottom
                chunk.set_relative_block(
                    x,
                    0,
                    z,
                    REGISTRY
                        .blocks
                        .get_default_state_id(vanilla_blocks::GRASS_BLOCK),
                );
            }
        }
    }

    fn build_surface(&self, _chunk: &ChunkAccess) {}

    fn apply_carvers(&self, _chunk: &ChunkAccess) {}

    fn apply_biome_decorations(&self, _chunk: &ChunkAccess) {}
}
