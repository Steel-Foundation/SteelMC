use std::sync::Arc;

use crate::chunk::{
    chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder, chunk_pyramid::ChunkStep,
};
use crate::worldgen::generator::context::WorldGenContext;
use crate::worldgen::generator::{CarversPhase, ChunkGenerator, GenerationChunk};

pub(crate) fn generate(
    context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let chunk = GenerationChunk::<CarversPhase>::acquire(&holder);

    context.generator.apply_carvers(chunk);
    // Generator-specific implementations normally consume their own state. This
    // central boundary also covers skipped work and future custom generators.
    chunk.clear_post_noise_state();
}
