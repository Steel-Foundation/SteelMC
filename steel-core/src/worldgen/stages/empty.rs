use std::sync::Arc;

use crate::chunk::{
    chunk_access::ChunkAccess,
    chunk_generation_task::StaticCache2D,
    chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
    proto_chunk::ProtoChunk,
    section::{ChunkSection, Sections},
};
use crate::worldgen::context::WorldGenContext;

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

    let proto_chunk = ProtoChunk::new(
        Sections::from_owned(sections),
        holder.get_pos(),
        context.min_y(),
        context.height(),
    );

    // Use no_notify variant so apply_step can notify through the completion channel.
    holder.insert_chunk_no_notify(ChunkAccess::Proto(proto_chunk));
}
