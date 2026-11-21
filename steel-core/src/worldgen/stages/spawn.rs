use std::sync::Arc;

use steel_registry::vanilla_game_rules::SPAWN_MOBS;

use crate::chunk::{
    chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder, chunk_pyramid::ChunkStep,
};
use crate::worldgen::generator::ChunkGenerator;
use crate::worldgen::generator::context::WorldGenContext;
use crate::worldgen::region::WorldGenRegion;
use crate::worldgen::spawn::spawn_original_mobs;

pub(crate) fn generate(
    context: Arc<WorldGenContext>,
    step: &ChunkStep,
    cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let Some(biome_zoom_seed) = context.generator.original_mob_spawn_biome_seed() else {
        return;
    };

    let world = context.world();
    let world_seed = {
        let level_data = world.level_data.read();
        if !world.get_game_rule_with_guard(&SPAWN_MOBS, &level_data) {
            return;
        }
        level_data.data().seed
    };

    let center = holder.get_pos();
    let region_random = context
        .generator
        .create_worldgen_region_random(world_seed, center);

    let mut region = WorldGenRegion::new(&context, step, cache, center, region_random);
    spawn_original_mobs(&mut region, &world, world_seed, biome_zoom_seed);
}
