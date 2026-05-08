use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use steel_registry::structure::TerrainAdjustment;
use steel_utils::ChunkPos;

use crate::chunk::{
    chunk_access::ChunkStatus, chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
};
use crate::world::structure::StructureStart;
use crate::worldgen::context::WorldGenContext;
use crate::worldgen::generator::ChunkGenerator;
use crate::worldgen::noise::beardifier::Beardifier;

pub(crate) fn generate(
    context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let chunk = holder
        .try_chunk(ChunkStatus::Biomes)
        .expect("Chunk not found at status Biomes");

    // Resolve the chunk's structure references into the starts the beardifier needs.
    // Mirrors vanilla's `StructureManager.startsForStructure(chunkPos, predicate)` —
    // walking each (structure, source-chunk-pos) pair, loading the source chunk's
    // STRUCTURE_STARTS entry, and copying it out.
    let pos = chunk.pos();
    let chunk_x = pos.0.x;
    let chunk_z = pos.0.y;

    let references = chunk.structure_references();

    let mut source_positions: FxHashSet<ChunkPos> = FxHashSet::default();
    for source_chunks in references.values() {
        source_positions.extend(source_chunks.iter().copied());
    }

    let beardifier = if source_positions.is_empty() {
        None
    } else {
        // Acquire all source chunks at STRUCTURE_STARTS up front. References can land on
        // chunks that haven't reached that status yet (rare, edge of generation region) —
        // skip those, vanilla does the same.
        let source_holders: Vec<Arc<ChunkHolder>> = source_positions
            .iter()
            .map(|p| Arc::clone(cache.get(p.0.x, p.0.y)))
            .collect();
        let source_chunks: Vec<_> = source_holders
            .iter()
            .filter_map(|h| h.try_chunk(ChunkStatus::StructureStarts))
            .collect();
        let mut source_indices: FxHashMap<ChunkPos, usize> = FxHashMap::default();
        let mut starts_guards = Vec::with_capacity(source_chunks.len());
        for source_chunk in &source_chunks {
            let source_pos = source_chunk.pos();
            source_indices.insert(source_pos, starts_guards.len());
            starts_guards.push(source_chunk.structure_starts());
        }

        // Resolve each (structure_id, source_pos) pair to a borrowed `&StructureStart`.
        // The starts_guards keep the underlying maps alive across this collection.
        let mut starts: Vec<&StructureStart> = Vec::new();
        for (structure_id, source_chunks_ref) in references.iter() {
            for &source_pos in source_chunks_ref {
                let Some(&guard_index) = source_indices.get(&source_pos) else {
                    continue;
                };
                let guard = &starts_guards[guard_index];
                if let Some(start) = guard.get(structure_id)
                    && start.chunk_pos == source_pos
                    && start.terrain_adjustment != TerrainAdjustment::None
                {
                    starts.push(start);
                }
            }
        }

        if starts.is_empty() {
            None
        } else {
            let b = Beardifier::for_structures_in_chunk(starts.iter().copied(), chunk_x, chunk_z);
            (!b.is_empty()).then_some(b)
        }
    };

    drop(references);

    context
        .generator
        .fill_from_noise(&chunk, beardifier.as_ref());
}
