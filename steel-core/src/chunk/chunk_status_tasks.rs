#![allow(missing_docs)]

use std::sync::Arc;

use steel_registry::{REGISTRY, multi_noise::get_overworld_biome_cached};
use steel_utils::Identifier;
use steel_utils::density::EvalCache;

use crate::chunk::{
    chunk_access::{ChunkAccess, ChunkStatus},
    chunk_generation_task::StaticCache2D,
    chunk_generator::ChunkGenerator,
    chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
    proto_chunk::ProtoChunk,
    section::{ChunkSection, Sections},
    world_gen_context::WorldGenContext,
};

pub struct ChunkStatusTasks;

/// All these functions are blocking.
impl ChunkStatusTasks {
    pub fn empty(
        context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
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

        //log::info!("Inserted proto chunk for {:?}", holder.get_pos());

        // Use no_notify variant - the caller (apply_step) will notify via the completion channel
        // to avoid rayon threads contending on tokio's scheduler mutex
        holder.insert_chunk_no_notify(ChunkAccess::Proto(proto_chunk));
        Ok(())
    }

    /// Generates structure starts.
    ///
    /// # Panics
    /// Panics if the chunk is not at `ChunkStatus::Empty` or higher.
    pub fn generate_structure_starts(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn generate_structure_references(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn load_structure_starts(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    /// # Panics
    /// Panics if the chunk is not at `ChunkStatus::StructureReferences` or higher.
    pub fn generate_biomes(
        context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        let chunk = holder
            .try_chunk(ChunkStatus::StructureReferences)
            .expect("Chunk not found at status StructureReferences");

        let pos = chunk.pos();
        let min_y = context.min_y();
        let section_count = context.section_count();

        let chunk_x = pos.0.x;
        let chunk_z = pos.0.y;

        // Per-chunk biome lookup cache
        let mut biome_cache: Option<usize> = None;
        // Density function evaluation cache (FlatCache, CacheOnce)
        let mut eval_cache = EvalCache::new();

        // Sample biomes for each section
        for section_index in 0..section_count {
            let section_y = (min_y / 16) + section_index as i32;
            let section = &chunk.sections().sections[section_index];
            let mut section_guard = section.write();

            // For each biome position in the section (4x4x4)
            for local_quart_x in 0..4i32 {
                for local_quart_y in 0..4i32 {
                    for local_quart_z in 0..4i32 {
                        // Calculate global quart position
                        let quart_x = chunk_x * 4 + local_quart_x;
                        let quart_y = section_y * 4 + local_quart_y;
                        let quart_z = chunk_z * 4 + local_quart_z;

                        // Sample climate at this quart position
                        let target = context.climate_sampler.sample(
                            quart_x,
                            quart_y,
                            quart_z,
                            &mut eval_cache,
                        );

                        // Get the biome for this climate
                        let biome_name = get_overworld_biome_cached(&target, &mut biome_cache);

                        // Convert biome name to ID (strip "minecraft:" prefix if present)
                        let biome_path = biome_name
                            .strip_prefix("minecraft:")
                            .unwrap_or(biome_name)
                            .to_string();
                        let biome_id = REGISTRY
                            .biomes
                            .id_from_key(&Identifier::vanilla(biome_path))
                            .unwrap_or(0) as u8;

                        section_guard.biomes.set(
                            local_quart_x as usize,
                            local_quart_y as usize,
                            local_quart_z as usize,
                            biome_id,
                        );
                    }
                }
            }
            drop(section_guard);
        }

        chunk.mark_dirty();

        Ok(())
    }

    #[allow(clippy::missing_panics_doc)]
    pub fn generate_noise(
        context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        let chunk = holder
            .try_chunk(ChunkStatus::Biomes)
            .expect("Chunk not found at status Biomes");
        context.generator.fill_from_noise(&chunk);
        Ok(())
    }

    pub fn generate_surface(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn generate_carvers(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn generate_features(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn initialize_light(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn light(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn generate_spawn(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn full(
        context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        //panic!("Full task");
        //log::info!("Chunk {:?} upgraded to full", holder.get_pos());
        holder.upgrade_to_full(context.weak_world());
        Ok(())
    }
}
