use std::sync::Arc;

use crate::chunk::{
    chunk_access::ChunkStatus, chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
};
use crate::worldgen::context::WorldGenContext;
use crate::worldgen::generator::ChunkGenerator;

#[expect(
    clippy::similar_names,
    reason = "quart coordinate names intentionally mirror x/y/z axes"
)]
pub(crate) fn generate(
    context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let chunk = holder
        .try_chunk(ChunkStatus::Noise)
        .expect("Chunk not found at status Noise");

    let min_qy = chunk.min_y() >> 2;
    let total_quarts_y = (chunk.sections().sections.len() * 4) as i32;

    let neighbor_biomes = |qx: i32, qy: i32, qz: i32| -> u16 {
        let chunk_x = qx >> 2;
        let chunk_z = qz >> 2;
        let neighbor = cache.get(chunk_x, chunk_z);
        let neighbor_chunk = neighbor
            .try_chunk(ChunkStatus::Biomes)
            .expect("Neighbor not at Biomes status");
        let sections = neighbor_chunk.sections();
        let local_qx = (qx - chunk_x * 4) as usize;
        let local_qz = (qz - chunk_z * 4) as usize;
        let qy_clamped = (qy - min_qy).clamp(0, total_quarts_y - 1) as usize;
        let section_idx = qy_clamped / 4;
        let local_qy = qy_clamped % 4;
        sections.sections[section_idx]
            .read()
            .biomes
            .get(local_qx, local_qy, local_qz)
    };

    context.generator.build_surface(&chunk, &neighbor_biomes);
}
