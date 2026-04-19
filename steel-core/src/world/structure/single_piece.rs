//! Simple "single piece" structures: one piece at chunk origin with fixed
//! size + random horizontal rotation, plus biome/height checks.
//!
//! Covers:
//! - Desert pyramid (21×15×21)
//! - Jungle temple (12×10×15)
//! - Swamp hut (7×7×9)
//! - Buried treasure (1×1×1 — no rotation, placed at `(chunkMinX+9, 90, chunkMinZ+9)`)

use steel_utils::density::DimensionNoises;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Identifier};

use crate::world::structure::placement::StructureSelectionEntry;
use crate::world::structure::{GenerationContext, GenerationStub, Structure, StructurePiece};

const fn make_single_piece_bb(
    chunk_min_x: i32,
    y: i32,
    chunk_min_z: i32,
    z_axis: bool,
    w: i32,
    h: i32,
    d: i32,
) -> BoundingBox {
    // Vanilla's `StructurePiece.makeBoundingBox` with direction's axis:
    //   N/S (axis Z): (x..x+w-1, y..y+h-1, z..z+d-1)
    //   E/W (axis X): (x..x+d-1, y..y+h-1, z..z+w-1)
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

/// Desert pyramid, jungle temple, swamp hut: one piece at (chunkMinX, 64, chunkMinZ)
/// with random horizontal rotation and a lowest-corner height check.
pub struct SinglePieceStructure {
    /// Piece template dimensions (width, height, depth).
    pub size: (i32, i32, i32),
    /// Piece-type identifier matching vanilla's `StructurePieceType` registry
    /// (lowercased: e.g. `"tedp"` for desert pyramid, `"tejp"` for jungle,
    /// `"tesh"` for swamp hut).
    pub piece_id: &'static str,
    /// If `true`, reject when `lowest < sea_level`.
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
            // Lowest-corner height check at the piece footprint's corners.
            let h0 = ctx.base_height(ctx.chunk_min_x, ctx.chunk_min_z, false) - 1;
            let h1 = ctx.base_height(ctx.chunk_min_x, ctx.chunk_min_z + d, false) - 1;
            let h2 = ctx.base_height(ctx.chunk_min_x + w, ctx.chunk_min_z, false) - 1;
            let h3 = ctx.base_height(ctx.chunk_min_x + w, ctx.chunk_min_z + d, false) - 1;
            let lowest = h0.min(h1).min(h2).min(h3);
            if lowest < ctx.sea_level {
                return None;
            }
        }

        // Biome check at chunk center, surface Y.
        let biome = ctx.biome_at(ctx.center_block_x, ctx.surface_y, ctx.center_block_z);
        if !entry.allowed_biomes.contains(&biome.key) {
            return None;
        }

        // Consume rotation RNG (vanilla's getRandomHorizontalDirection):
        // N=0, E=1, S=2, W=3 — the BB orientation only cares whether the
        // piece's Z axis is the world's Z axis (N/S) or X axis (E/W).
        let dir_idx = rng.next_i32_bounded(4);
        let z_axis = matches!(dir_idx, 0 | 2);
        let bb = make_single_piece_bb(ctx.chunk_min_x, 64, ctx.chunk_min_z, z_axis, w, h, d);

        Some(GenerationStub {
            position: (ctx.center_block_x, ctx.surface_y, ctx.center_block_z),
            pieces: vec![StructurePiece {
                piece_type: Identifier::new_static("minecraft", self.piece_id),
                bounding_box: bb,
                gen_depth: 0,
                orientation: None,
                nbt_data: Vec::new(),
                ground_level_delta: 0,
                junctions: Vec::new(),
            }],
        })
    }
}

/// Buried treasure: single 1×1×1 piece at `(chunkMinX+9, 90, chunkMinZ+9)`.
/// Biome check at ocean-floor Y, not surface.
pub struct BuriedTreasureStructure;

impl<N: DimensionNoises> Structure<N> for BuriedTreasureStructure {
    fn find_generation_point(
        &self,
        ctx: &mut GenerationContext<'_, '_, N>,
        entry: &StructureSelectionEntry,
        _rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        // Ocean floor height at chunk center (OCEAN_FLOOR_WG: opaque = Solid only).
        let ocean_floor_y = ctx.base_height(ctx.center_block_x, ctx.center_block_z, true) - 1;

        let biome = ctx.biome_at(ctx.center_block_x, ocean_floor_y, ctx.center_block_z);
        if !entry.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let x = ctx.chunk_min_x + 9;
        let z = ctx.chunk_min_z + 9;
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
