#![expect(
    missing_docs,
    reason = "task functions are named after their vanilla counterparts"
)]

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

        // Use no_notify variant - the caller (apply_step) will notify via the completion channel
        // to avoid rayon threads contending on tokio's scheduler mutex
        holder.insert_chunk_no_notify(ChunkAccess::Proto(proto_chunk));
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
    ) {
    }

    pub fn generate_structure_references(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) {
    }

    pub fn load_structure_starts(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) {
    }

    /// # Panics
    /// Panics if the chunk is not at `ChunkStatus::StructureReferences` or higher.
    pub fn generate_biomes(
        context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        let chunk = holder
            .try_chunk(ChunkStatus::StructureReferences)
            .expect("Chunk not found at status StructureReferences");

        context.generator.create_biomes(&chunk);
    }

    #[expect(
        clippy::missing_panics_doc,
        reason = "panic is unreachable given correct status ordering"
    )]
    pub fn generate_noise(
        context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        let chunk = holder
            .try_chunk(ChunkStatus::Biomes)
            .expect("Chunk not found at status Biomes");
        context.generator.fill_from_noise(&chunk);
    }

    /// # Panics
    /// Panics if the chunk has not reached `ChunkStatus::Noise`.
    #[expect(
        clippy::similar_names,
        reason = "chunk_x/chunk_z and local_qx/local_qz are intentionally similar"
    )]
    pub fn generate_surface(
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

    // TODO: Wire up to context.generator.apply_carvers() once carver generation is implemented
    pub fn generate_carvers(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) {
    }

    // TODO: Wire up to context.generator.apply_biome_decorations() once feature generation is implemented
    pub fn generate_features(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) {
    }

    pub fn initialize_light(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) {
    }

    pub fn light(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) {
    }

    pub fn generate_spawn(
        _context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        _holder: Arc<ChunkHolder>,
    ) {
    }

    pub fn full(
        context: Arc<WorldGenContext>,
        _step: &ChunkStep,
        _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        //log::info!("Chunk {:?} upgraded to full", holder.get_pos());
        holder.upgrade_to_full(context.weak_world());
    }
}
