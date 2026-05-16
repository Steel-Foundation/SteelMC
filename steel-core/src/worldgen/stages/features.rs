use std::sync::Arc;

use crate::chunk::{
    chunk_access::ChunkStatus, chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
};
use crate::worldgen::context::WorldGenContext;
use crate::worldgen::generator::ChunkGenerator;
use crate::worldgen::region::WorldGenRegion;

pub(crate) fn generate(
    context: Arc<WorldGenContext>,
    step: &ChunkStep,
    cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let center = holder.get_pos();

    let Some(chunk) = holder.try_chunk(ChunkStatus::Carvers) else {
        panic!("Chunk not found at status Carvers");
    };
    chunk.prime_final_heightmaps();
    drop(chunk);

    let world_seed = context.world().seed();
    let region_random = context
        .generator
        .create_worldgen_region_random(world_seed, center);
    let mut region = WorldGenRegion::new(context.as_ref(), step, cache, center, region_random);
    context.generator.apply_biome_decorations(&mut region);
}
