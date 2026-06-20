//! Jungle temple structure start generation.

use steel_registry::structure::StructureData;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{Direction, Identifier};

use crate::structure::{
    GenerationStub, ProceduralPieceData, Structure, StructureGenerationContext, StructurePiece,
    StructurePiecePayload, make_oriented_piece_bounding_box, random_horizontal_direction,
};

pub(crate) const JUNGLE_TEMPLE_WIDTH: i32 = 12;
pub(crate) const JUNGLE_TEMPLE_HEIGHT: i32 = 10;
pub(crate) const JUNGLE_TEMPLE_DEPTH: i32 = 15;

/// Runtime state for vanilla `JungleTemplePiece`.
#[derive(Debug, Clone, Default)]
pub struct JungleTemplePieceData {
    /// Vanilla `ScatteredFeaturePiece.heightPosition`; `None` means not height-adjusted yet.
    pub height_position: Option<i32>,
    /// Whether the main chest has already been placed.
    pub placed_main_chest: bool,
    /// Whether the hidden chest has already been placed.
    pub placed_hidden_chest: bool,
    /// Whether the first arrow-dispenser trap has already been placed.
    pub placed_trap1: bool,
    /// Whether the second arrow-dispenser trap has already been placed.
    pub placed_trap2: bool,
}

impl JungleTemplePieceData {
    /// Creates a fresh jungle temple piece payload.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            height_position: None,
            placed_main_chest: false,
            placed_hidden_chest: false,
            placed_trap1: false,
            placed_trap2: false,
        }
    }
}

/// Vanilla's `JungleTempleStructure` / `SinglePieceStructure`.
pub struct JungleTempleStructure;

const fn jungle_temple_piece(west: i32, north: i32, orientation: Direction) -> StructurePiece {
    StructurePiece {
        piece_type: Identifier::new_static("minecraft", "tejp"),
        bounding_box: make_oriented_piece_bounding_box(
            west,
            64,
            north,
            orientation,
            JUNGLE_TEMPLE_WIDTH,
            JUNGLE_TEMPLE_HEIGHT,
            JUNGLE_TEMPLE_DEPTH,
        ),
        gen_depth: 0,
        orientation: Some(orientation),
        payload: StructurePiecePayload::Procedural(ProceduralPieceData::JungleTemple(
            JungleTemplePieceData::new(),
        )),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    }
}

impl Structure for JungleTempleStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let x0 = ctx.chunk_min_x();
        let z0 = ctx.chunk_min_z();
        let h0 = ctx.base_height(x0, z0, false) - 1;
        let h1 = ctx.base_height(x0, z0 + JUNGLE_TEMPLE_DEPTH, false) - 1;
        let h2 = ctx.base_height(x0 + JUNGLE_TEMPLE_WIDTH, z0, false) - 1;
        let h3 = ctx.base_height(x0 + JUNGLE_TEMPLE_WIDTH, z0 + JUNGLE_TEMPLE_DEPTH, false) - 1;
        if h0.min(h1).min(h2).min(h3) < ctx.sea_level() {
            return None;
        }

        let center_y = ctx.base_height(ctx.center_block_x(), ctx.center_block_z(), false) - 1;
        let biome = ctx.biome_at(ctx.center_block_x(), center_y, ctx.center_block_z());
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let orientation = random_horizontal_direction(rng);
        Some(GenerationStub {
            position: (ctx.center_block_x(), center_y, ctx.center_block_z()),
            pieces: vec![jungle_temple_piece(
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
    fn jungle_temple_piece_uses_full_procedural_payload() {
        let piece = jungle_temple_piece(16, 32, Direction::South);

        assert_eq!(
            piece.piece_type,
            Identifier::new_static("minecraft", "tejp")
        );
        assert_eq!(piece.gen_depth, 0);
        assert_eq!(piece.orientation, Some(Direction::South));
        let StructurePiecePayload::Procedural(ProceduralPieceData::JungleTemple(data)) =
            piece.payload
        else {
            panic!("jungle temple piece should use procedural payload");
        };
        assert_eq!(data.height_position, None);
        assert!(!data.placed_main_chest);
        assert!(!data.placed_hidden_chest);
        assert!(!data.placed_trap1);
        assert!(!data.placed_trap2);
    }
}
