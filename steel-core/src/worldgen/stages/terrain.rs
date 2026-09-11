use std::sync::Arc;

use glam::IVec3;
use rustc_hash::FxHashMap;
use steel_registry::structure::TerrainAdjustment;
use steel_utils::{ChunkPos, Identifier};
use steel_worldgen::noise::Beardifier;
use steel_worldgen::structure::StructureStart;

use crate::chunk::{
    chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder, chunk_pyramid::ChunkStep,
    status::ChunkStatus,
};
use crate::worldgen::generator::context::WorldGenContext;
use crate::worldgen::generator::{ChunkGenerator, GenerationChunk, TerrainPhase};

type StructureReferencesForTerrain = Vec<(Identifier, Vec<ChunkPos>)>;

pub(crate) fn generate(
    context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let (chunk_x, chunk_z, references) = collect_structure_references(holder.as_ref());
    let beardifier = build_beardifier(cache, &references, chunk_x, chunk_z);
    let chunk = GenerationChunk::<TerrainPhase>::acquire(&holder);

    let min_quart_y = chunk.min_y() >> 2;
    let quart_height = (chunk.section_count() * 4) as i32;
    let neighbor_biomes = |quart_pos: IVec3| -> u16 {
        let neighbor_chunk_x = quart_pos.x >> 2;
        let neighbor_chunk_z = quart_pos.z >> 2;
        let neighbor = cache.get(neighbor_chunk_x, neighbor_chunk_z);
        let neighbor_chunk = neighbor
            .try_chunk(ChunkStatus::Biomes)
            .expect("neighbor chunk is not at Biomes status");
        let sections = neighbor_chunk.sections();
        let local_quart_x = (quart_pos.x - neighbor_chunk_x * 4) as usize;
        let local_quart_z = (quart_pos.z - neighbor_chunk_z * 4) as usize;
        let local_quart_y = (quart_pos.y - min_quart_y).clamp(0, quart_height - 1) as usize;
        let section_index = local_quart_y / 4;
        sections.sections[section_index].read().biomes.get(
            local_quart_x,
            local_quart_y % 4,
            local_quart_z,
        )
    };

    context
        .generator
        .build_terrain(chunk, beardifier.as_ref(), &neighbor_biomes);
}

fn collect_structure_references(holder: &ChunkHolder) -> (i32, i32, StructureReferencesForTerrain) {
    let chunk = holder
        .try_chunk(ChunkStatus::Biomes)
        .expect("chunk is not at Biomes status");

    let pos = chunk.pos();
    let references = chunk
        .structure_references()
        .iter()
        .map(|(structure_id, source_chunks)| {
            (
                structure_id.clone(),
                source_chunks.iter().copied().collect::<Vec<_>>(),
            )
        })
        .collect();

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

    let source_holders = source_positions
        .iter()
        .map(|pos| Arc::clone(cache.get(pos.0.x, pos.0.y)))
        .collect::<Vec<_>>();
    let source_chunks = source_holders
        .iter()
        .filter_map(|holder| holder.try_chunk(ChunkStatus::StructureStarts))
        .collect::<Vec<_>>();
    let mut source_indices = FxHashMap::default();
    let mut starts_guards = Vec::with_capacity(source_chunks.len());
    for source_chunk in &source_chunks {
        source_indices.insert(source_chunk.pos(), starts_guards.len());
        starts_guards.push(source_chunk.structure_starts());
    }

    let mut starts = Vec::<&StructureStart>::new();
    for (structure_id, source_chunks) in references {
        for source_pos in source_chunks {
            let Some(&guard_index) = source_indices.get(source_pos) else {
                continue;
            };
            let guard = &starts_guards[guard_index];
            if let Some(start) = guard.get(structure_id)
                && start.chunk_pos == *source_pos
                && start.terrain_adjustment != TerrainAdjustment::None
            {
                starts.push(start);
            }
        }
    }

    (!starts.is_empty())
        .then(|| Beardifier::for_structures_in_chunk(starts, chunk_x, chunk_z))
        .filter(|beardifier| !beardifier.is_empty())
}
