use std::sync::Arc;

use crate::chunk::{
    chunk_access::ChunkStatus, chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
};
use crate::worldgen::context::WorldGenContext;
use crate::worldgen::generator::ChunkGenerator;

/// Generates structure starts.
///
/// # Panics
/// Panics if the chunk is not at `ChunkStatus::Empty` or higher.
pub(crate) fn generate_starts(
    context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let chunk = holder
        .try_chunk(ChunkStatus::Empty)
        .expect("Chunk not found at status Empty");

    context.generator.create_structures(&chunk);
}

/// Collects structure references from surrounding chunks' starts.
///
/// # Panics
/// Panics if the chunk is not at `ChunkStatus::StructureStarts` or higher.
pub(crate) fn generate_references(
    _context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let chunk = holder
        .try_chunk(ChunkStatus::StructureStarts)
        .expect("Chunk not found at status StructureStarts");
    let target_pos = chunk.pos();
    let target_x = target_pos.0.x;
    let target_z = target_pos.0.y;
    let target_block_x = target_x * 16;
    let target_block_z = target_z * 16;
    drop(chunk);

    // Radius-8 scan for starts whose BB intersects this chunk.
    for source_x in (target_x - 8)..=(target_x + 8) {
        for source_z in (target_z - 8)..=(target_z + 8) {
            let source_holder = cache.get(source_x, source_z);
            let Some(source_chunk) = source_holder.try_chunk(ChunkStatus::StructureStarts) else {
                continue;
            };

            for (structure_id, start) in source_chunk.structure_starts().iter() {
                // Empty-pieces starts have no BB (legacy/unknown types).
                // `start.bounding_box` is already inflated by `bb_inflate`.
                let Some(bb) = start.bounding_box else {
                    continue;
                };
                if bb.intersects_xz(
                    target_block_x,
                    target_block_z,
                    target_block_x + 15,
                    target_block_z + 15,
                ) {
                    let target_chunk = holder
                        .try_chunk(ChunkStatus::StructureStarts)
                        .expect("Chunk not found");
                    target_chunk
                        .structure_references_mut()
                        .entry(structure_id.clone())
                        .or_default()
                        .push(steel_utils::ChunkPos::new(source_x, source_z));
                    // Source-start reference counts are updated at serialization time.
                }
            }
        }
    }
}

pub(crate) fn load_starts(
    _context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    _holder: Arc<ChunkHolder>,
) {
}
