//! prepared chunks for exercising materialization independently of terrain gen

use std::sync::Arc;

use steel_registry::vanilla_blocks;
use steel_utils::{ChunkPos, SectionPos};

use crate::chunk::{
    Chunk,
    chunk_generation_task::StaticCache2D,
    chunk_holder::ChunkHolder,
    chunk_pyramid::GENERATION_PYRAMID,
    section::{ChunkSection, Sections},
    status::ChunkStatus,
};
use crate::world::World;

/// decoration seed that selects pigs from the mc plains table at the origin
pub const PLAINS_SPAWN_SEED: i64 = 4096;
const GRASS_SURFACE_Y: i32 = 64;

/// sets up a flat grass surface under open sky with the spawn dependencies in place
pub fn prepare_plains_spawn(cache: &StaticCache2D<Arc<ChunkHolder>>, world: &Arc<World>) {
    let center = ChunkPos::new(0, 0);
    let radius = GENERATION_PYRAMID
        .get_step_to(ChunkStatus::Spawn)
        .direct_dependencies
        .get_radius() as i32;
    let width = SectionPos::section_to_block_coord(1, 0) as usize;

    for x in -radius..=radius {
        for z in -radius..=radius {
            let holder = cache.get(x, z);
            let sections = (0..holder.height() as usize / width)
                .map(|_| ChunkSection::new_empty())
                .collect();

            let chunk = Chunk::new(
                Sections::from_owned(sections),
                holder.get_pos(),
                holder.min_y(),
                holder.height(),
                Arc::downgrade(world),
            );

            if holder.get_pos() == center {
                let relative_y = (GRASS_SURFACE_Y - holder.min_y()) as usize;
                let grass = vanilla_blocks::GRASS_BLOCK.default_state();
                let blocks = (0..width)
                    .flat_map(|x| (0..width).map(move |z| (x, relative_y, z, grass)))
                    .collect::<Vec<_>>();

                chunk.write_block_batch_for_generation(ChunkStatus::Features, &blocks);
                chunk.prime_final_heightmaps();
                holder.insert_chunk(chunk, ChunkStatus::Light);
            } else {
                holder.insert_chunk(chunk, ChunkStatus::Biomes);
            }
        }
    }
}
