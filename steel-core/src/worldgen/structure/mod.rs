//! Core chunk adapter for the worldgen structure engine.

pub(crate) mod piece_placer;

use crate::chunk::Chunk;
use steel_utils::Identifier;
use steel_worldgen::structure::StructureGenerationContext;

pub use steel_worldgen::structure::{
    FixedStructureBiomeProvider, StructureGenerator, StructureLocateCandidate, StructureLocatePlan,
    squared_distance,
};

/// Generates structure starts and writes them into the chunk.
pub fn create_structures(
    generator: &StructureGenerator,
    chunk: &Chunk,
    ctx: &mut dyn StructureGenerationContext,
) {
    let starts = generator.generate_starts_for_chunk(ctx, |structure: &Identifier| {
        chunk
            .structure_starts()
            .get(structure)
            .is_some_and(|start| !start.pieces.is_empty())
    });

    if starts.is_empty() {
        return;
    }

    {
        let mut chunk_starts = chunk.structure_starts_mut();
        for start in starts {
            chunk_starts.insert(start.structure.clone(), start);
        }
    }
    chunk.mark_dirty();
}
