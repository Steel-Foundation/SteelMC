#![allow(missing_docs)]

use std::sync::Arc;

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
        context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        let chunk = holder
            .try_chunk(ChunkStatus::Empty)
            .expect("Chunk not found at status Empty");

        context.generator.create_structures(&chunk);
        Ok(())
    }

    pub fn generate_structure_references(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        let chunk = holder
            .try_chunk(ChunkStatus::StructureStarts)
            .expect("Chunk not found at status StructureStarts");
        let target_pos = chunk.pos();
        let target_x = target_pos.0.x;
        let target_z = target_pos.0.y;
        let target_block_x = target_x * 16;
        let target_block_z = target_z * 16;
        drop(chunk);

        // Scan radius 8 around the target chunk for structure starts
        // whose bounding boxes intersect this chunk's area.
        for source_x in (target_x - 8)..=(target_x + 8) {
            for source_z in (target_z - 8)..=(target_z + 8) {
                let source_holder = cache.get(source_x, source_z);
                let Some(source_chunk) = source_holder.try_chunk(ChunkStatus::StructureStarts)
                else {
                    continue;
                };

                let starts = source_chunk.structure_starts();
                for (structure_id, start) in starts.iter() {
                    if start.pieces.is_empty() {
                        continue;
                    }

                    // Compute the overall bounding box of all pieces
                    let mut bb = start.pieces[0].bounding_box;
                    for piece in &start.pieces[1..] {
                        bb = steel_utils::BoundingBox::new(
                            bb.min_x.min(piece.bounding_box.min_x),
                            bb.min_y.min(piece.bounding_box.min_y),
                            bb.min_z.min(piece.bounding_box.min_z),
                            bb.max_x.max(piece.bounding_box.max_x),
                            bb.max_y.max(piece.bounding_box.max_y),
                            bb.max_z.max(piece.bounding_box.max_z),
                        );
                    }

                    // Vanilla inflates the BB when terrain_adaptation != NONE
                    let inflate = start.bb_inflate;
                    if bb.intersects_xz(
                        target_block_x - inflate,
                        target_block_z - inflate,
                        target_block_x + 15 + inflate,
                        target_block_z + 15 + inflate,
                    ) {
                        let target_chunk = holder
                            .try_chunk(ChunkStatus::StructureStarts)
                            .expect("Chunk not found");
                        target_chunk
                            .structure_references_mut()
                            .entry(structure_id.clone())
                            .or_default()
                            .push(steel_utils::ChunkPos::new(source_x, source_z));

                        // Increment the reference count on the source start
                        drop(target_chunk);
                        // Note: reference count updates on the source chunk's start
                        // are handled during serialization, not here.
                    }
                }
            }
        }
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

        context.generator.create_biomes(&chunk);

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

    /// # Panics
    /// Panics if the chunk has not reached `ChunkStatus::Noise`.
    #[allow(clippy::similar_names)]
    pub fn generate_surface(
        context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
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
        Ok(())
    }

    // TODO: Wire up to context.generator.apply_carvers() once carver generation is implemented
    pub fn generate_carvers(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    // TODO: Wire up to context.generator.apply_biome_decorations() once feature generation is implemented
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
        //log::info!("Chunk {:?} upgraded to full", holder.get_pos());
        holder.upgrade_to_full(context.weak_world());
        Ok(())
    }
}
