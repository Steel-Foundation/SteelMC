//! Desert pyramid start generation and piece runtime state.

use steel_registry::structure::StructureData;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BlockPos, Direction, Identifier};

use crate::structure::{
    GenerationStub, ProceduralPieceData, Structure, StructureGenerationContext, StructurePiece,
    StructurePiecePayload, make_oriented_piece_bounding_box, random_horizontal_direction,
};

pub(crate) const DESERT_PYRAMID_WIDTH: i32 = 21;
pub(crate) const DESERT_PYRAMID_HEIGHT: i32 = 15;
pub(crate) const DESERT_PYRAMID_DEPTH: i32 = 21;

/// Runtime state for vanilla `DesertPyramidPiece`.
#[derive(Debug, Clone)]
pub struct DesertPyramidPieceData {
    /// Vanilla `ScatteredFeaturePiece.heightPosition`; `None` means not height-adjusted yet.
    pub height_position: Option<i32>,
    /// Chest placement flags ordered by `Direction.get2DDataValue`.
    pub has_placed_chest: [bool; 4],
    /// Per-run archaeology candidates collected by `postProcess`; vanilla does not persist these.
    pub potential_suspicious_sand_world_positions: Vec<BlockPos>,
    /// Per-run collapsed-roof archaeology position collected by `postProcess`.
    pub random_collapsed_roof_pos: BlockPos,
}

impl DesertPyramidPieceData {
    /// Creates the initial runtime state stored on a newly generated desert pyramid piece.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            height_position: None,
            has_placed_chest: [false; 4],
            potential_suspicious_sand_world_positions: Vec::new(),
            random_collapsed_roof_pos: BlockPos::new(0, 0, 0),
        }
    }
}

impl Default for DesertPyramidPieceData {
    fn default() -> Self {
        Self::new()
    }
}

/// Vanilla's `DesertPyramidStructure`.
pub struct DesertPyramidStructure;

const fn desert_pyramid_piece(
    chunk_min_x: i32,
    chunk_min_z: i32,
    orientation: Direction,
) -> StructurePiece {
    StructurePiece {
        piece_type: Identifier::new_static("minecraft", "tedp"),
        bounding_box: make_oriented_piece_bounding_box(
            chunk_min_x,
            64,
            chunk_min_z,
            orientation,
            DESERT_PYRAMID_WIDTH,
            DESERT_PYRAMID_HEIGHT,
            DESERT_PYRAMID_DEPTH,
        ),
        gen_depth: 0,
        orientation: Some(orientation),
        payload: StructurePiecePayload::Procedural(ProceduralPieceData::DesertPyramid(
            DesertPyramidPieceData::new(),
        )),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    }
}

impl Structure for DesertPyramidStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let (x0, z0) = (ctx.chunk_min_x(), ctx.chunk_min_z());
        let h0 = ctx.base_height(x0, z0, false) - 1;
        let h1 = ctx.base_height(x0, z0 + DESERT_PYRAMID_DEPTH, false) - 1;
        let h2 = ctx.base_height(x0 + DESERT_PYRAMID_WIDTH, z0, false) - 1;
        let h3 = ctx.base_height(x0 + DESERT_PYRAMID_WIDTH, z0 + DESERT_PYRAMID_DEPTH, false) - 1;
        if h0.min(h1).min(h2).min(h3) < ctx.sea_level() {
            return None;
        }

        let surface_y = ctx.surface_y();
        let biome = ctx.biome_at(ctx.center_block_x(), surface_y, ctx.center_block_z());
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let orientation = random_horizontal_direction(rng);
        Some(GenerationStub {
            position: (ctx.center_block_x(), surface_y, ctx.center_block_z()),
            pieces: vec![desert_pyramid_piece(
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
    use steel_utils::BoundingBox;

    #[test]
    fn desert_pyramid_piece_uses_full_procedural_payload() {
        let piece = desert_pyramid_piece(32, -48, Direction::East);

        assert_eq!(
            piece.piece_type,
            Identifier::new_static("minecraft", "tedp")
        );
        assert_eq!(piece.gen_depth, 0);
        assert_eq!(piece.orientation, Some(Direction::East));
        assert_eq!(
            piece.bounding_box,
            BoundingBox::new(32, 64, -48, 52, 78, -28)
        );
        let StructurePiecePayload::Procedural(ProceduralPieceData::DesertPyramid(data)) =
            piece.payload
        else {
            panic!("desert pyramid should use its procedural payload");
        };
        assert_eq!(data.height_position, None);
        assert_eq!(data.has_placed_chest, [false; 4]);
        assert!(data.potential_suspicious_sand_world_positions.is_empty());
        assert_eq!(data.random_collapsed_roof_pos, BlockPos::new(0, 0, 0));
    }
}
