use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::{BlockStateId, Identifier};

use crate::chunk::chunk_access::ChunkAccess;
use crate::worldgen::generator::ChunkGenerator;

/// A chunk generator that generates a flat world.
///
/// Uses a fixed biome (plains) for all positions, matching vanilla's
/// `FlatLevelSource` with `FixedBiomeSource`.
pub struct FlatChunkGenerator {
    /// Block layers from world bottom upwards.
    pub layers: Vec<BlockStateId>,
    /// The biome ID for plains (cached at construction).
    biome_id: u16,
}

impl FlatChunkGenerator {
    /// Creates a new `FlatChunkGenerator`.
    #[must_use]
    pub fn new(bedrock: BlockStateId, dirt: BlockStateId, grass: BlockStateId) -> Self {
        Self::new_layers(vec![bedrock, dirt, dirt, grass])
    }

    /// Creates a new flat generator with explicit block layers from bottom upwards.
    #[must_use]
    pub fn new_layers(layers: Vec<BlockStateId>) -> Self {
        let biome_id = REGISTRY
            .biomes
            .id_from_key(&Identifier::vanilla("plains".to_string()))
            .unwrap_or(0) as u16;

        Self { layers, biome_id }
    }
}

impl ChunkGenerator for FlatChunkGenerator {
    fn spawn_height(&self, min_y: i32, height: i32) -> i32 {
        min_y + height.min(self.layers.len() as i32)
    }

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
        let max_relative_y = chunk.sections().sections.len() * 16;

        for x in 0..16 {
            for z in 0..16 {
                for (relative_y, block) in self.layers.iter().enumerate().take(max_relative_y) {
                    chunk.set_relative_block(x, relative_y, z, *block);
                }
            }
        }
    }

    fn build_surface(&self, _chunk: &ChunkAccess, _neighbor_biomes: &dyn Fn(i32, i32, i32) -> u16) {
    }

    fn apply_carvers(&self, _chunk: &ChunkAccess) {}

    fn apply_biome_decorations(&self, _chunk: &ChunkAccess) {}
}
