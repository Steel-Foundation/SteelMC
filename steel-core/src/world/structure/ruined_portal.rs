//! Ruined portal. Mirrors vanilla's `RuinedPortalStructure.findGenerationPoint`
//! RNG consumption to determine the biome-check Y. Produces bounding box only.

use steel_registry::structure::{
    RuinedPortalPlacementData, RuinedPortalSetupData, StructureConfigData, StructureData,
};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Direction, Identifier, Rotation};

use crate::world::structure::{
    GenerationStub, Structure, StructureGenerationContext, StructurePiece,
};

/// Template sizes for `portal_1`..`portal_10`.
const PORTAL_SIZES: [(i32, i32, i32); 10] = [
    (6, 10, 6),
    (9, 12, 9),
    (8, 9, 9),
    (8, 9, 9),
    (10, 10, 7),
    (5, 7, 7),
    (9, 7, 9),
    (14, 9, 9),
    (10, 8, 9),
    (12, 8, 10),
];

/// Template sizes for `giant_portal_1`..`giant_portal_3`.
const GIANT_PORTAL_SIZES: [(i32, i32, i32); 3] = [(11, 17, 16), (11, 16, 16), (16, 16, 16)];

/// Terrain query operations needed by the ruined portal generation.
pub enum TerrainQuery {
    /// Get surface height at (x, z). Returns first solid Y from top.
    SurfaceHeight {
        /// Block X.
        x: i32,
        /// Block Z.
        z: i32,
        /// `true` for `OCEAN_FLOOR_WG`, `false` for `WORLD_SURFACE_WG`.
        ocean_floor: bool,
    },
    /// Check if block at (x, y, z) is opaque for the selected heightmap.
    IsOpaque {
        /// Block X.
        x: i32,
        /// Block Y.
        y: i32,
        /// Block Z.
        z: i32,
        /// `true` for `OCEAN_FLOOR_WG`, `false` for `WORLD_SURFACE_WG`.
        ocean_floor: bool,
    },
}

/// Result of a terrain query.
pub enum TerrainResult {
    /// Surface height result.
    Height(i32),
    /// Block opacity result.
    Opaque(bool),
}

/// Result of ruined portal generation point computation.
pub struct PortalResult {
    /// Biome check position `(block_x, block_y, block_z)`.
    pub biome_check_pos: (i32, i32, i32),
    /// Bounding box of the placed portal piece.
    pub bounding_box: BoundingBox,
}

/// Matches vanilla's `RuinedPortalStructure.findGenerationPoint`.
#[expect(
    clippy::too_many_lines,
    reason = "inlines vanilla's setup → size → rotation → mirror → placement pipeline"
)]
pub fn find_generation_point(
    rng: &mut LegacyRandom,
    chunk_x: i32,
    chunk_z: i32,
    setups: &[RuinedPortalSetupData],
    min_y: i32,
    terrain: &mut dyn FnMut(TerrainQuery) -> TerrainResult,
) -> PortalResult {
    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;

    // Weighted selection via nextFloat.
    let setup = if setups.len() > 1 {
        let total: f32 = setups.iter().map(|s| s.weight).sum();
        let mut pick = rng.next_f32();
        let mut chosen_idx = setups.len() - 1;
        for (i, s) in setups.iter().enumerate() {
            pick -= s.weight / total;
            if pick < 0.0 {
                chosen_idx = i;
                break;
            }
        }
        &setups[chosen_idx]
    } else {
        &setups[0]
    };

    // Vanilla `sample(rng, p)` short-circuits at 0.0/1.0; we keep the guard so
    // out-of-range values added later don't unexpectedly draw RNG.
    let air_pocket = if setup.air_pocket_probability <= 0.0 {
        false
    } else if setup.air_pocket_probability >= 1.0 {
        true
    } else {
        rng.next_f32() < setup.air_pocket_probability
    };

    // 5% giant, 95% regular.
    let (sx, sy, sz) = if rng.next_f32() < 0.05 {
        GIANT_PORTAL_SIZES[rng.next_i32_bounded(GIANT_PORTAL_SIZES.len() as i32) as usize]
    } else {
        PORTAL_SIZES[rng.next_i32_bounded(PORTAL_SIZES.len() as i32) as usize]
    };

    let rotation = Rotation::get_random(rng);
    let mirror_front_back = rng.next_f32() >= 0.5;
    let pivot_x = sx / 2;
    let pivot_z = sz / 2;
    let bb = rotation.get_bounding_box_full(
        (base_x, 0, base_z),
        (sx, sy, sz),
        pivot_x,
        pivot_z,
        mirror_front_back,
    );
    // Vanilla's `BoundingBox.getCenter()` = min + (max - min + 1) / 2, which
    // differs from (min + max) / 2 for even spans due to integer rounding.
    let bb_center_x = bb.min_x + (bb.max_x - bb.min_x + 1) / 2;
    let bb_center_z = bb.min_z + (bb.max_z - bb.min_z + 1) / 2;
    let ocean_floor = matches!(setup.placement, RuinedPortalPlacementData::OnOceanFloor);
    let surface_y = match terrain(TerrainQuery::SurfaceHeight {
        x: bb_center_x,
        z: bb_center_z,
        ocean_floor,
    }) {
        TerrainResult::Height(h) => h,
        TerrainResult::Opaque(_) => unreachable!(),
    } - 1;

    let min_y_threshold = min_y + 15;
    let new_y = match setup.placement {
        RuinedPortalPlacementData::OnLandSurface | RuinedPortalPlacementData::OnOceanFloor => {
            surface_y
        }
        RuinedPortalPlacementData::Underground => {
            let max_y = surface_y - sy;
            if min_y_threshold < max_y {
                rng.next_i32_between(min_y_threshold, max_y)
            } else {
                max_y
            }
        }
        RuinedPortalPlacementData::InMountain => {
            let max_y = surface_y - sy;
            if 70 < max_y {
                rng.next_i32_between(70, max_y)
            } else {
                max_y
            }
        }
        RuinedPortalPlacementData::PartlyBuried => surface_y - sy + rng.next_i32_between(2, 8),
        RuinedPortalPlacementData::InNether => {
            if air_pocket {
                rng.next_i32_between(32, 100)
            } else if rng.next_f32() < 0.5 {
                rng.next_i32_between(27, 29)
            } else {
                rng.next_i32_between(29, 100)
            }
        }
    };

    // findSuitableY: scan down, break when ≥3 of 4 corners are opaque.
    let corners = [
        (bb.min_x, bb.min_z),
        (bb.max_x, bb.min_z),
        (bb.min_x, bb.max_z),
        (bb.max_x, bb.max_z),
    ];
    let mut projected_y = new_y;
    'scan: while projected_y > min_y_threshold {
        let mut solid_count = 0;
        for &(cx, cz) in &corners {
            if matches!(
                terrain(TerrainQuery::IsOpaque {
                    x: cx,
                    y: projected_y,
                    z: cz,
                    ocean_floor,
                }),
                TerrainResult::Opaque(true)
            ) {
                solid_count += 1;
                if solid_count == 3 {
                    break 'scan;
                }
            }
        }
        projected_y -= 1;
    }

    PortalResult {
        biome_check_pos: (base_x, projected_y, base_z),
        bounding_box: rotation.get_bounding_box_full(
            (base_x, projected_y, base_z),
            (sx, sy, sz),
            pivot_x,
            pivot_z,
            mirror_front_back,
        ),
    }
}

/// Registered under `"minecraft:ruined_portal"` and its biome variants
/// (desert / jungle / mountain / ocean / swamp / nether). The terrain closure
/// creates a fresh aquifer + column cache per query since piece gen can probe
/// outside this chunk.
pub struct RuinedPortalStructure;

impl Structure for RuinedPortalStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let mut terrain = |q: TerrainQuery| -> TerrainResult {
            match q {
                TerrainQuery::SurfaceHeight { x, z, ocean_floor } => {
                    TerrainResult::Height(ctx.terrain_surface_height(x, z, ocean_floor))
                }
                TerrainQuery::IsOpaque {
                    x,
                    y,
                    z,
                    ocean_floor,
                } => TerrainResult::Opaque(ctx.terrain_is_opaque(x, y, z, ocean_floor)),
            }
        };

        let StructureConfigData::RuinedPortal { setups } = &structure.config else {
            return None;
        };
        if setups.is_empty() {
            return None;
        }

        let result = find_generation_point(
            rng,
            ctx.chunk_x(),
            ctx.chunk_z(),
            setups,
            ctx.min_y(),
            &mut terrain,
        );

        let (bx, by, bz) = result.biome_check_pos;
        let biome = ctx.biome_at(bx, by, bz);
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        Some(GenerationStub {
            position: result.biome_check_pos,
            pieces: vec![StructurePiece::non_jigsaw(
                Identifier::new_static("minecraft", "rupo"),
                result.bounding_box,
                0,
                Some(Direction::North),
            )],
        })
    }
}
