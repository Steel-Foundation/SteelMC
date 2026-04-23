//! "Single piece" structures: one piece at chunk origin with fixed size + random
//! horizontal rotation. Desert pyramid (21×15×21), jungle temple (12×10×15),
//! swamp hut (7×7×9), buried treasure (1×1×1 at `(chunkMinX+9, 90, chunkMinZ+9)`).

use steel_utils::density::DimensionNoises;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Identifier};

use crate::world::structure::placement::StructureSelectionEntry;
use crate::world::structure::{GenerationContext, GenerationStub, Structure, StructurePiece};

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

impl<N: DimensionNoises> Structure<N> for SinglePieceStructure {
    fn find_generation_point(
        &self,
        ctx: &mut GenerationContext<'_, '_, N>,
        entry: &StructureSelectionEntry,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let (w, h, d) = self.size;

        if self.require_above_sea {
            let (x0, z0) = (ctx.chunk_min_x, ctx.chunk_min_z);
            let lowest = [(x0, z0), (x0, z0 + d), (x0 + w, z0), (x0 + w, z0 + d)]
                .into_iter()
                .map(|(x, z)| ctx.base_height(x, z, false) - 1)
                .min()
                .unwrap();
            if lowest < ctx.sea_level {
                return None;
            }
        }

        let surface_y = ctx.surface_y();
        let biome = ctx.biome_at(ctx.center_block_x, surface_y, ctx.center_block_z);
        if !entry.allowed_biomes.contains(&biome.key) {
            return None;
        }

        // Vanilla's getRandomHorizontalDirection: N=0, E=1, S=2, W=3.
        let z_axis = matches!(rng.next_i32_bounded(4), 0 | 2);
        Some(GenerationStub {
            position: (ctx.center_block_x, surface_y, ctx.center_block_z),
            pieces: vec![StructurePiece {
                piece_type: Identifier::new_static("minecraft", self.piece_id),
                bounding_box: make_single_piece_bb(
                    ctx.chunk_min_x,
                    64,
                    ctx.chunk_min_z,
                    z_axis,
                    w,
                    h,
                    d,
                ),
                gen_depth: 0,
                orientation: None,
                nbt_data: Vec::new(),
                ground_level_delta: 0,
                junctions: Vec::new(),
            }],
        })
    }
}

/// Single 1×1×1 piece at `(chunkMinX+9, 90, chunkMinZ+9)`. Biome check at ocean-floor Y.
pub struct BuriedTreasureStructure;

impl<N: DimensionNoises> Structure<N> for BuriedTreasureStructure {
    fn find_generation_point(
        &self,
        ctx: &mut GenerationContext<'_, '_, N>,
        entry: &StructureSelectionEntry,
        _rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let ocean_floor_y = ctx.base_height(ctx.center_block_x, ctx.center_block_z, true) - 1;
        let biome = ctx.biome_at(ctx.center_block_x, ocean_floor_y, ctx.center_block_z);
        if !entry.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let (x, z) = (ctx.chunk_min_x + 9, ctx.chunk_min_z + 9);
        Some(GenerationStub {
            position: (x, 90, z),
            pieces: vec![StructurePiece {
                piece_type: Identifier::new_static("minecraft", "btp"),
                bounding_box: BoundingBox::new(x, 90, z, x, 90, z),
                gen_depth: 0,
                orientation: None,
                nbt_data: Vec::new(),
                ground_level_delta: 0,
                junctions: Vec::new(),
            }],
        })
    }
}
