use std::sync::Arc;

use crate::chunk::{
    chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder, chunk_pyramid::ChunkStep,
};
use crate::worldgen::context::WorldGenContext;

pub(crate) fn generate(
    _context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    _holder: Arc<ChunkHolder>,
) {
}
