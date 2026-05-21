use std::sync::Arc;

use rustc_hash::FxHashMap;
use steel_registry::structure::TerrainAdjustment;
use steel_utils::{ChunkPos, Identifier};

use crate::chunk::{
    chunk_access::ChunkStatus, chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
};
use crate::world::structure::StructureStart;
use crate::worldgen::context::WorldGenContext;
use crate::worldgen::generator::ChunkGenerator;
use crate::worldgen::noise::beardifier::Beardifier;

type StructureReferencesForNoise = Vec<(Identifier, Vec<ChunkPos>)>;

pub(crate) fn generate(
    context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let (chunk_x, chunk_z, references) = collect_structure_references(holder.as_ref());
    let beardifier = build_beardifier(cache, &references, chunk_x, chunk_z);

    let chunk = holder
        .try_chunk(ChunkStatus::Biomes)
        .expect("Chunk not found at status Biomes");
    context
        .generator
        .fill_from_noise(&chunk, beardifier.as_ref());
}

fn collect_structure_references(holder: &ChunkHolder) -> (i32, i32, StructureReferencesForNoise) {
    let chunk = holder
        .try_chunk(ChunkStatus::Biomes)
        .expect("Chunk not found at status Biomes");

    let pos = chunk.pos();
    let references = chunk.structure_references();
    let references = references
        .iter()
        .map(|(structure_id, source_chunks)| {
            (
                structure_id.clone(),
                source_chunks.iter().copied().collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();

    (pos.0.x, pos.0.y, references)
}

fn build_beardifier(
    cache: &StaticCache2D<Arc<ChunkHolder>>,
    references: &[(Identifier, Vec<ChunkPos>)],
    chunk_x: i32,
    chunk_z: i32,
) -> Option<Beardifier> {
    let mut source_positions = references
        .iter()
        .flat_map(|(_, source_chunks)| source_chunks.iter().copied())
        .collect::<Vec<_>>();
    if source_positions.is_empty() {
        return None;
    }

    source_positions.sort_by_key(|pos| (pos.0.x, pos.0.y));
    source_positions.dedup();

    // Acquire referenced chunks without holding the center chunk lock. The
    // position order prevents cross-chunk read cycles when writers are queued.
    let source_holders = source_positions
        .iter()
        .map(|p| Arc::clone(cache.get(p.0.x, p.0.y)))
        .collect::<Vec<_>>();
    let source_chunks = source_holders
        .iter()
        .filter_map(|h| h.try_chunk(ChunkStatus::StructureStarts))
        .collect::<Vec<_>>();
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
    for (structure_id, source_chunks_ref) in references {
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
        let beardifier =
            Beardifier::for_structures_in_chunk(starts.iter().copied(), chunk_x, chunk_z);
        (!beardifier.is_empty()).then_some(beardifier)
    }
}
