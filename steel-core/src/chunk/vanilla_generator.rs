use steel_registry::{REGISTRY, vanilla_blocks};

use crate::chunk::chunk_access::ChunkAccess;
use crate::chunk::chunk_generator::ChunkGenerator;
use crate::worldgen::BiomeSourceKind;

/// A chunk generator for vanilla (normal) world generation.
///
/// Matches vanilla's `NoiseBasedChunkGenerator`. The biome source is pluggable
/// per-dimension — overworld, nether, and end each provide a different
/// [`BiomeSourceKind`] variant.
pub struct VanillaGenerator {
    /// Biome source for this dimension. Determines biomes at each quart position.
    biome_source: BiomeSourceKind,
}

impl VanillaGenerator {
    /// Creates a new `VanillaGenerator` with the given biome source.
    #[must_use]
    pub const fn new(biome_source: BiomeSourceKind) -> Self {
        Self { biome_source }
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

        let mut sampler = self.biome_source.chunk_sampler();

        // Column-major iteration: sample all Y values for each (X, Z) column
        // before moving to the next column. This keeps the column cache effective —
        // column-level density functions (continents, erosion, ridges, etc.) are
        // computed once per column instead of once per sample.
        for local_quart_x in 0..4i32 {
            for local_quart_z in 0..4i32 {
                let quart_x = chunk_x * 4 + local_quart_x;
                let quart_z = chunk_z * 4 + local_quart_z;

                for section_index in 0..section_count {
                    let section_y = (min_y / 16) + section_index as i32;
                    let section = &chunk.sections().sections[section_index];
                    let mut section_guard = section.write();

                    for local_quart_y in 0..4i32 {
                        let quart_y = section_y * 4 + local_quart_y;

                        let biome = sampler.sample(quart_x, quart_y, quart_z);
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
        }

        chunk.mark_dirty();
    }

    fn fill_from_noise(&self, chunk: &ChunkAccess) {
        // TODO: Implement actual noise-based terrain generation (NoiseChunk + trilinear interpolation)
        for x in 0..16 {
            for z in 0..16 {
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
