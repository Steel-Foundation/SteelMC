//! Swamp hut structure start generation.

use steel_registry::structure::StructureData;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{Direction, Identifier};

use crate::structure::{
    GenerationStub, ProceduralPieceData, Structure, StructureGenerationContext, StructurePiece,
    StructurePiecePayload, make_oriented_piece_bounding_box, random_horizontal_direction,
};

pub(crate) const SWAMP_HUT_WIDTH: i32 = 7;
pub(crate) const SWAMP_HUT_HEIGHT: i32 = 7;
pub(crate) const SWAMP_HUT_DEPTH: i32 = 9;

/// Runtime state for vanilla `SwampHutPiece`.
#[derive(Debug, Clone, Default)]
pub struct SwampHutPieceData {
    /// Vanilla `ScatteredFeaturePiece.heightPosition`; `None` means not height-adjusted yet.
    pub height_position: Option<i32>,
    /// Whether the structure witch has already been spawned.
    pub spawned_witch: bool,
    /// Whether the structure black cat has already been spawned.
    pub spawned_cat: bool,
}

impl SwampHutPieceData {
    /// Creates a fresh swamp hut piece payload.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            height_position: None,
            spawned_witch: false,
            spawned_cat: false,
        }
    }
}

/// Vanilla's `SwampHutStructure`.
pub struct SwampHutStructure;

const fn swamp_hut_piece(west: i32, north: i32, orientation: Direction) -> StructurePiece {
    StructurePiece {
        piece_type: Identifier::new_static("minecraft", "tesh"),
        bounding_box: make_oriented_piece_bounding_box(
            west,
            64,
            north,
            orientation,
            SWAMP_HUT_WIDTH,
            SWAMP_HUT_HEIGHT,
            SWAMP_HUT_DEPTH,
        ),
        gen_depth: 0,
        orientation: Some(orientation),
        payload: StructurePiecePayload::Procedural(ProceduralPieceData::SwampHut(
            SwampHutPieceData::new(),
        )),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    }
}

impl Structure for SwampHutStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let center_y = ctx.base_height(ctx.center_block_x(), ctx.center_block_z(), false) - 1;
        let biome = ctx.biome_at(ctx.center_block_x(), center_y, ctx.center_block_z());
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let orientation = random_horizontal_direction(rng);
        Some(GenerationStub {
            position: (ctx.center_block_x(), center_y, ctx.center_block_z()),
            pieces: vec![swamp_hut_piece(
                ctx.chunk_min_x(),
                ctx.chunk_min_z(),
                orientation,
            )],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swamp_hut_piece_uses_full_procedural_payload() {
        let piece = swamp_hut_piece(16, 32, Direction::West);

        assert_eq!(
            piece.piece_type,
            Identifier::new_static("minecraft", "tesh")
        );
        assert_eq!(piece.gen_depth, 0);
        assert_eq!(piece.orientation, Some(Direction::West));
        let StructurePiecePayload::Procedural(ProceduralPieceData::SwampHut(data)) = piece.payload
        else {
            panic!("swamp hut piece should use procedural payload");
        };
        assert_eq!(data.height_position, None);
        assert!(!data.spawned_witch);
        assert!(!data.spawned_cat);
    }
}
