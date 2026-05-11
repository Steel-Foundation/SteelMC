//! "Single piece" structures: one piece at chunk origin with fixed size + random
//! horizontal rotation. Desert pyramid (21×15×21), jungle temple (12×10×15),
//! swamp hut (7×7×9), buried treasure (1×1×1 at `(chunkMinX+9, 90, chunkMinZ+9)`).

use steel_registry::structure::StructureData;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Direction, Identifier};

use crate::world::structure::{
    GenerationStub, Structure, StructureGenerationContext, StructurePiece,
    random_horizontal_direction,
};

/// Vanilla's `StructurePiece.makeBoundingBox`: N/S keep (w,d); E/W swap to (d,w).
const fn make_single_piece_bb(
    chunk_min_x: i32,
    y: i32,
    chunk_min_z: i32,
    z_axis: bool,
    w: i32,
    h: i32,
    d: i32,
) -> BoundingBox {
    let (bw, bd) = if z_axis { (w, d) } else { (d, w) };
    BoundingBox::new(
        chunk_min_x,
        y,
        chunk_min_z,
        chunk_min_x + bw - 1,
        y + h - 1,
        chunk_min_z + bd - 1,
    )
}

/// Desert pyramid / jungle temple / swamp hut: one piece at `(chunkMinX, 64, chunkMinZ)`
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
        let z_axis = matches!(orientation, Direction::North | Direction::South);
        Some(GenerationStub {
            position: (ctx.center_block_x(), surface_y, ctx.center_block_z()),
            pieces: vec![StructurePiece::non_jigsaw(
                Identifier::new_static("minecraft", self.piece_id),
                make_single_piece_bb(ctx.chunk_min_x(), 64, ctx.chunk_min_z(), z_axis, w, h, d),
                0,
                Some(orientation),
            )],
        })
    }
}

/// Single 1×1×1 piece at `(chunkMinX+9, 90, chunkMinZ+9)`. Biome check at ocean-floor Y.
pub struct BuriedTreasureStructure;

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
            pieces: vec![StructurePiece::non_jigsaw(
                Identifier::new_static("minecraft", "btp"),
                BoundingBox::new(x, 90, z, x, 90, z),
                0,
                None,
            )],
        })
    }
}
