//! "Single piece" structures: one piece at chunk origin with fixed size + random
//! horizontal rotation. Jungle temple (12×10×15), swamp hut (7×7×9), and
//! buried treasure (1×1×1 at `(chunkMinX+9, 90, chunkMinZ+9)`).

use steel_registry::structure::StructureData;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Identifier};

use crate::world::structure::{
    GenerationStub, ProceduralPieceData, Structure, StructureGenerationContext, StructurePiece,
    StructurePiecePayload, make_oriented_piece_bounding_box, random_horizontal_direction,
};

/// Jungle temple / swamp hut: one piece at `(chunkMinX, 64, chunkMinZ)`
/// with random rotation and a lowest-corner height check.
pub struct SinglePieceStructure {
    /// Template dimensions (width, height, depth).
    pub size: (i32, i32, i32),
    /// Vanilla `StructurePieceType` id (`"tedp"`, `"tejp"`, `"tesh"`, ...).
    pub piece_id: &'static str,
    /// If `true`, reject when any footprint corner is below `sea_level`.
    pub require_above_sea: bool,
}

impl Structure for SinglePieceStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let (w, h, d) = self.size;

        if self.require_above_sea {
            let (x0, z0) = (ctx.chunk_min_x(), ctx.chunk_min_z());
            let h0 = ctx.base_height(x0, z0, false) - 1;
            let h1 = ctx.base_height(x0, z0 + d, false) - 1;
            let h2 = ctx.base_height(x0 + w, z0, false) - 1;
            let h3 = ctx.base_height(x0 + w, z0 + d, false) - 1;
            if h0.min(h1).min(h2).min(h3) < ctx.sea_level() {
                return None;
            }
        }

        let surface_y = ctx.surface_y();
        let biome = ctx.biome_at(ctx.center_block_x(), surface_y, ctx.center_block_z());
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let orientation = random_horizontal_direction(rng);
        Some(GenerationStub {
            position: (ctx.center_block_x(), surface_y, ctx.center_block_z()),
            pieces: vec![StructurePiece::non_jigsaw(
                Identifier::new_static("minecraft", self.piece_id),
                make_oriented_piece_bounding_box(
                    ctx.chunk_min_x(),
                    64,
                    ctx.chunk_min_z(),
                    orientation,
                    w,
                    h,
                    d,
                ),
                0,
                Some(orientation),
            )],
        })
    }
}

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
