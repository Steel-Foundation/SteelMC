//! Ruined portal. Mirrors vanilla's `RuinedPortalStructure.findGenerationPoint`
//! RNG consumption to determine the biome-check Y. Produces bounding box only.

use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Direction, Identifier, Rotation};
use steel_worldgen::density::{ColumnCache, DimensionNoises, NoiseSettings};

use crate::world::structure::placement::StructureSelectionEntry;
use crate::world::structure::{GenerationContext, GenerationStub, Structure, StructurePiece};
use crate::worldgen::generators::vanilla::{
    column_interpolated_density, iterate_noise_column_with_aquifer,
};
use crate::worldgen::noise::aquifer::{Aquifer, AquiferResult};

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

/// Vertical placement type from the setup config.
#[derive(Debug, Clone, Copy)]
enum Placement {
    OnLandSurface,
    PartlyBuried,
    Underground,
    InMountain,
    OnOceanFloor,
    InNether,
}

/// A setup entry from the structure JSON.
struct Setup {
    placement: Placement,
    weight: f32,
    air_pocket_prob: f32,
}

/// Setups for a given ruined portal variant.
fn get_setups(structure_path: &str) -> Vec<Setup> {
    let mk = |placement, weight, air_pocket_prob| Setup {
        placement,
        weight,
        air_pocket_prob,
    };
    match structure_path {
        "ruined_portal" => vec![
            mk(Placement::Underground, 0.5, 1.0),
            mk(Placement::OnLandSurface, 0.5, 0.5),
        ],
        "ruined_portal_desert" => vec![mk(Placement::PartlyBuried, 1.0, 0.0)],
        "ruined_portal_jungle" => vec![mk(Placement::OnLandSurface, 1.0, 0.5)],
        "ruined_portal_mountain" => vec![
            mk(Placement::InMountain, 0.5, 1.0),
            mk(Placement::OnLandSurface, 0.5, 0.5),
        ],
        "ruined_portal_ocean" | "ruined_portal_swamp" => {
            vec![mk(Placement::OnOceanFloor, 1.0, 0.0)]
        }
        "ruined_portal_nether" => vec![mk(Placement::InNether, 1.0, 0.5)],
        _ => vec![mk(Placement::OnLandSurface, 1.0, 0.0)],
    }
}

/// Terrain query operations needed by the ruined portal generation.
pub enum TerrainQuery {
    /// Get surface height at (x, z). Returns first solid Y from top.
    SurfaceHeight(i32, i32),
    /// Check if block at (x, y, z) is opaque for `WORLD_SURFACE_WG` heightmap.
    IsOpaque(i32, i32, i32),
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
    structure_path: &str,
    min_y: i32,
    terrain: &mut dyn FnMut(TerrainQuery) -> TerrainResult,
) -> PortalResult {
    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;
    let setups = get_setups(structure_path);

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
    let air_pocket = if setup.air_pocket_prob <= 0.0 {
        false
    } else if setup.air_pocket_prob >= 1.0 {
        true
    } else {
        rng.next_f32() < setup.air_pocket_prob
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
    let surface_y = match terrain(TerrainQuery::SurfaceHeight(bb_center_x, bb_center_z)) {
        TerrainResult::Height(h) => h,
        TerrainResult::Opaque(_) => unreachable!(),
    } - 1;

    let min_y_threshold = min_y + 15;
    let new_y = match setup.placement {
        Placement::OnLandSurface | Placement::OnOceanFloor => surface_y,
        Placement::Underground => {
            let max_y = surface_y - sy;
            if min_y_threshold < max_y {
                rng.next_i32_between(min_y_threshold, max_y)
            } else {
                max_y
            }
        }
        Placement::InMountain => {
            let max_y = surface_y - sy;
            if 70 < max_y {
                rng.next_i32_between(70, max_y)
            } else {
                max_y
            }
        }
        Placement::PartlyBuried => surface_y - sy + rng.next_i32_between(2, 8),
        Placement::InNether => {
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
                terrain(TerrainQuery::IsOpaque(cx, projected_y, cz)),
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

impl<N: DimensionNoises> Structure<N> for RuinedPortalStructure {
    fn find_generation_point(
        &self,
        ctx: &mut GenerationContext<'_, '_, N>,
        entry: &StructureSelectionEntry,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let noises = ctx.noises;
        let splitter = ctx.splitter;
        let cell_w = N::Settings::CELL_WIDTH;
        let cell_h = N::Settings::CELL_HEIGHT;

        let mut terrain = |q: TerrainQuery| -> TerrainResult {
            let (qx, qz) = match q {
                TerrainQuery::SurfaceHeight(x, z) | TerrainQuery::IsOpaque(x, _, z) => (x, z),
            };
            let cell_x = qx.div_euclid(cell_w) * cell_w;
            let cell_z = qz.div_euclid(cell_w) * cell_w;
            let aq_chunk_x = (cell_x >> 4) * 16;
            let aq_chunk_z = (cell_z >> 4) * 16;
            let aq_cache = N::ColumnCache::default();
            let mut fresh_aq = Aquifer::<N>::new(
                aq_chunk_x,
                aq_chunk_z,
                N::Settings::MIN_Y,
                N::Settings::HEIGHT,
                splitter,
                noises,
                aq_cache,
            );
            let mut fresh_cache = N::ColumnCache::default();
            fresh_cache.init_grid(aq_chunk_x, aq_chunk_z, noises);
            match q {
                TerrainQuery::SurfaceHeight(x, z) => {
                    TerrainResult::Height(iterate_noise_column_with_aquifer::<N>(
                        &mut fresh_cache,
                        noises,
                        &mut fresh_aq,
                        x,
                        z,
                        false,
                    ))
                }
                TerrainQuery::IsOpaque(x, y, z) => {
                    let density = column_interpolated_density::<N>(
                        &mut fresh_cache,
                        noises,
                        x,
                        y,
                        z,
                        cell_w,
                        cell_h,
                    );
                    let opaque = match fresh_aq.compute_substance(noises, x, y, z, density) {
                        AquiferResult::Solid | AquiferResult::Fluid(_) => true,
                        AquiferResult::Air => false,
                    };
                    TerrainResult::Opaque(opaque)
                }
            }
        };

        let result = find_generation_point(
            rng,
            ctx.chunk_x,
            ctx.chunk_z,
            &entry.structure.path,
            N::Settings::MIN_Y,
            &mut terrain,
        );

        let (bx, by, bz) = result.biome_check_pos;
        let biome = ctx.biome_at(bx, by, bz);
        if !entry.allowed_biomes.contains(&biome.key) {
            return None;
        }

        Some(GenerationStub {
            position: result.biome_check_pos,
            pieces: vec![StructurePiece {
                piece_type: Identifier::new_static("minecraft", "rupo"),
                bounding_box: result.bounding_box,
                gen_depth: 0,
                orientation: Some(Direction::North),
                nbt_data: Vec::new(),
                ground_level_delta: 0,
                junctions: Vec::new(),
                projection: None,
            }],
        })
    }
}
