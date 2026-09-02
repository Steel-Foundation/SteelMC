use std::sync::Arc;

use crate::chunk::{
    Chunk,
    chunk_generation_task::StaticCache2D,
    chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
    section::{ChunkSection, Sections},
};
use crate::worldgen::generator::context::WorldGenContext;

pub(crate) fn generate(
    context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let sections = (0..context.section_count())
        .map(|_| ChunkSection::new_empty())
        .collect::<Vec<_>>()
        .into_boxed_slice();

    let proto_chunk = Chunk::new(
        Sections::from_owned(sections),
        holder.get_pos(),
        context.min_y(),
        context.height(),
        context.weak_world(),
    );

    // Use no_notify variant so apply_step can notify through the completion channel.
    holder.insert_chunk_no_notify(proto_chunk);
}
