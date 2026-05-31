//! Single-piece buried treasure structure start generation.

use steel_registry::structure::StructureData;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Identifier};

use crate::structure::{
    GenerationStub, ProceduralPieceData, Structure, StructureGenerationContext, StructurePiece,
    StructurePiecePayload,
};

/// Single 1×1×1 piece at `(chunkMinX+9, 90, chunkMinZ+9)`. Biome check at ocean-floor Y.
pub struct BuriedTreasureStructure;

const fn buried_treasure_piece(x: i32, z: i32) -> StructurePiece {
    StructurePiece {
        piece_type: Identifier::new_static("minecraft", "btp"),
        bounding_box: BoundingBox::new(x, 90, z, x, 90, z),
        gen_depth: 0,
        orientation: None,
        payload: StructurePiecePayload::Procedural(ProceduralPieceData::BuriedTreasure),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    }
}

impl Structure for BuriedTreasureStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        _rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let ocean_floor_y = ctx.base_height(ctx.center_block_x(), ctx.center_block_z(), true) - 1;
        let biome = ctx.biome_at(ctx.center_block_x(), ocean_floor_y, ctx.center_block_z());
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let (x, z) = (ctx.chunk_min_x() + 9, ctx.chunk_min_z() + 9);
        Some(GenerationStub {
            position: (x, 90, z),
            pieces: vec![buried_treasure_piece(x, z)],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buried_treasure_piece_uses_procedural_payload() {
        let piece = buried_treasure_piece(25, -39);

        assert_eq!(piece.piece_type, Identifier::new_static("minecraft", "btp"));
        assert_eq!(
            piece.bounding_box,
            BoundingBox::new(25, 90, -39, 25, 90, -39)
        );
        assert_eq!(piece.gen_depth, 0);
        assert_eq!(piece.orientation, None);
        assert!(matches!(
            piece.payload,
            StructurePiecePayload::Procedural(ProceduralPieceData::BuriedTreasure)
        ));
    }
}
