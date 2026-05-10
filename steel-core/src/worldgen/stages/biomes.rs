use std::sync::Arc;

use crate::chunk::{
    chunk_access::ChunkStatus, chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
};
use crate::worldgen::context::WorldGenContext;
use crate::worldgen::generator::ChunkGenerator;

pub(crate) fn generate(
    context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let chunk = holder
        .try_chunk(ChunkStatus::StructureReferences)
        .expect("Chunk not found at status StructureReferences");

    context.generator.create_biomes(&chunk);
}
