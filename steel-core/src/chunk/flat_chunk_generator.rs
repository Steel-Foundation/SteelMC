use steel_registry::REGISTRY;
use steel_utils::{BlockStateId, Identifier};

use crate::chunk::{chunk_access::ChunkAccess, chunk_generator::ChunkGenerator};

/// A chunk generator that generates a flat world.
///
/// Uses a fixed biome (plains) for all positions, matching vanilla's
/// `FlatLevelSource` with `FixedBiomeSource`.
pub struct FlatChunkGenerator {
    /// The block state id for bedrock.
    pub bedrock: BlockStateId,
    /// The block state id for dirt.
    pub dirt: BlockStateId,
    /// The block state id for grass blocks.
    pub grass: BlockStateId,
    /// The biome ID for plains (cached at construction).
    biome_id: u16,
}

impl FlatChunkGenerator {
    /// Creates a new `FlatChunkGenerator`.
    #[must_use]
    pub fn new(bedrock: BlockStateId, dirt: BlockStateId, grass: BlockStateId) -> Self {
        let biome_id = REGISTRY
            .biomes
            .id_from_key(&Identifier::vanilla("plains".to_string()))
            .unwrap_or(0) as u16;

        Self {
            bedrock,
            dirt,
            grass,
            biome_id,
        }
    }
}

impl ChunkGenerator for FlatChunkGenerator {
    fn create_structures(&self, _chunk: &ChunkAccess) {}

    fn create_biomes(&self, chunk: &ChunkAccess) {
        let section_count = chunk.sections().sections.len();

        for section_index in 0..section_count {
            let section = &chunk.sections().sections[section_index];
            let mut section_guard = section.write();

            for local_quart_x in 0..4usize {
                for local_quart_y in 0..4usize {
                    for local_quart_z in 0..4usize {
                        section_guard.biomes.set(
                            local_quart_x,
                            local_quart_y,
                            local_quart_z,
                            self.biome_id,
                        );
                    }
                }
            }
            drop(section_guard);
        }

        chunk.mark_dirty();
    }

    fn fill_from_noise(&self, chunk: &ChunkAccess) {
        // Layers:
        // 0: Bedrock
        // 1-2: Dirt
        // 3: Grass Block
        // (Relative to bottom of chunk, assuming 0 is bottom)

        // TODO: Get actual height from level/config?
        // For now assuming standard height behavior where 0 is bottom of the chunk data.

        for x in 0..16 {
            for z in 0..16 {
                // Bedrock at bottom
                chunk.set_relative_block(x, 0, z, self.bedrock);

                // Dirt layers
                chunk.set_relative_block(x, 1, z, self.dirt);
                chunk.set_relative_block(x, 2, z, self.dirt);

                // Grass block
                chunk.set_relative_block(x, 3, z, self.grass);
            }
        }
    }

    fn build_surface(&self, _chunk: &ChunkAccess) {}

    fn apply_carvers(&self, _chunk: &ChunkAccess) {}

    fn apply_biome_decorations(&self, _chunk: &ChunkAccess) {}
}
