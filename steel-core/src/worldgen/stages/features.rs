use std::sync::Arc;

use crate::chunk::{
    chunk_access::ChunkStatus, chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
};
use crate::worldgen::context::WorldGenContext;
use crate::worldgen::generator::ChunkGenerator;
use crate::worldgen::region::WorldGenRegion;
use steel_utils::ChunkPos;

pub(crate) fn generate(
    context: Arc<WorldGenContext>,
    step: &ChunkStep,
    cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let center = holder.get_pos();
    prime_worldgen_heightmaps_for_carver_dependencies(step, cache.as_ref(), center);

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

fn prime_worldgen_heightmaps_for_carver_dependencies(
    step: &ChunkStep,
    cache: &StaticCache2D<Arc<ChunkHolder>>,
    center: ChunkPos,
) {
    let radius = step.direct_dependencies.get_radius_of(ChunkStatus::Carvers) as i32;

    for chunk_z in center.0.y - radius..=center.0.y + radius {
        for chunk_x in center.0.x - radius..=center.0.x + radius {
            let distance = (chunk_x - center.0.x)
                .abs()
                .max((chunk_z - center.0.y).abs()) as usize;
            if !step
                .direct_dependencies
                .get(distance)
                .is_some_and(|status| status >= ChunkStatus::Carvers)
            {
                continue;
            }

            let Some(chunk) = cache.get(chunk_x, chunk_z).try_chunk(ChunkStatus::Carvers) else {
                panic!(
                    "Features step missing Carvers dependency chunk ({chunk_x}, {chunk_z}) \
                     at distance {distance} from ({}, {})",
                    center.0.x, center.0.y
                );
            };
            chunk.prime_worldgen_heightmaps();
        }
    }
}
