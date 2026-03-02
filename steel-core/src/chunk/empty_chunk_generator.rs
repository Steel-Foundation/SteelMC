use crate::chunk::{chunk_access::ChunkAccess, chunk_generator::ChunkGenerator};

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
    fn create_structures(&self, _chunk: &ChunkAccess) {}

    fn create_biomes(&self, _chunk: &ChunkAccess) {}

    fn fill_from_noise(&self, _chunk: &ChunkAccess) {}

    fn build_surface(&self, _chunk: &ChunkAccess) {}

    fn apply_carvers(&self, _chunk: &ChunkAccess) {}

    fn apply_biome_decorations(&self, _chunk: &ChunkAccess) {}
}
