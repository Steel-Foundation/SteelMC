//! Ruined portal biome check position computation.
//!
//! Mirrors vanilla's `RuinedPortalStructure.findGenerationPoint` RNG consumption
//! to determine the correct Y for the biome check. Does not place actual blocks.

use steel_utils::density::{ColumnCache, DimensionNoises, NoiseSettings};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Identifier, Rotation};

use crate::chunk::aquifer::{Aquifer, AquiferResult};
use crate::chunk::vanilla_generator::{
    column_interpolated_density, iterate_noise_column_with_aquifer,
};
use crate::world::structure::placement::StructureSelectionEntry;
use crate::world::structure::{GenerationContext, GenerationStub, Structure, StructurePiece};

/// Template sizes (x, y, z) for regular portals (`portal_1` through `portal_10`).
const PORTAL_SIZES: [(i32, i32, i32); 10] = [
    (6, 10, 6),  // portal_1
    (9, 12, 9),  // portal_2
    (8, 9, 9),   // portal_3
    (8, 9, 9),   // portal_4
    (10, 10, 7), // portal_5
    (5, 7, 7),   // portal_6
    (9, 7, 9),   // portal_7
    (14, 9, 9),  // portal_8
    (10, 8, 9),  // portal_9
    (12, 8, 10), // portal_10
];

/// Template sizes (x, y, z) for giant portals (`giant_portal_1` through `giant_portal_3`).
const GIANT_PORTAL_SIZES: [(i32, i32, i32); 3] = [
    (11, 17, 16), // giant_portal_1
    (11, 16, 16), // giant_portal_2
    (16, 16, 16), // giant_portal_3
];

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

/// Returns the setups for a given ruined portal variant.
fn get_setups(structure_path: &str) -> Vec<Setup> {
    match structure_path {
        "ruined_portal" => vec![
            Setup {
                placement: Placement::Underground,
                weight: 0.5,
                air_pocket_prob: 1.0,
            },
            Setup {
                placement: Placement::OnLandSurface,
                weight: 0.5,
                air_pocket_prob: 0.5,
            },
        ],
        "ruined_portal_desert" => vec![Setup {
            placement: Placement::PartlyBuried,
            weight: 1.0,
            air_pocket_prob: 0.0,
        }],
        "ruined_portal_jungle" => vec![Setup {
            placement: Placement::OnLandSurface,
            weight: 1.0,
            air_pocket_prob: 0.5,
        }],
        "ruined_portal_mountain" => vec![
            Setup {
                placement: Placement::InMountain,
                weight: 0.5,
                air_pocket_prob: 1.0,
            },
            Setup {
                placement: Placement::OnLandSurface,
                weight: 0.5,
                air_pocket_prob: 0.5,
            },
        ],
        "ruined_portal_ocean" | "ruined_portal_swamp" => vec![Setup {
            placement: Placement::OnOceanFloor,
            weight: 1.0,
            air_pocket_prob: 0.0,
        }],
        "ruined_portal_nether" => vec![Setup {
            placement: Placement::InNether,
            weight: 1.0,
            air_pocket_prob: 0.5,
        }],
        _ => vec![Setup {
            placement: Placement::OnLandSurface,
            weight: 1.0,
            air_pocket_prob: 0.0,
        }],
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

/// Computes the biome check position and piece bounding box for a ruined portal.
///
/// Matches vanilla's `RuinedPortalStructure.findGenerationPoint`.
/// `terrain` handles both surface height queries and block opacity checks.
#[expect(
    clippy::too_many_lines,
    reason = "mirrors vanilla's RuinedPortalStructure.findGenerationPoint RNG order"
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

    // Select setup — matches vanilla's weighted selection using nextFloat
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

    let placement = setup.placement;

    // Air pocket: sample(random, probability)
    #[expect(
        clippy::float_cmp,
        reason = "air_pocket_prob is hardcoded {0.0, 0.5, 1.0} — exact compare mirrors vanilla's sample() fast path"
    )]
    let air_pocket = if setup.air_pocket_prob == 0.0 {
        false
    } else if setup.air_pocket_prob == 1.0 {
        true
    } else {
        rng.next_f32() < setup.air_pocket_prob
    };

    // Template selection: 5% giant, 95% regular
    let (sx, sy, sz) = if rng.next_f32() < 0.05 {
        let idx = rng.next_i32_bounded(GIANT_PORTAL_SIZES.len() as i32) as usize;
        GIANT_PORTAL_SIZES[idx]
    } else {
        let idx = rng.next_i32_bounded(PORTAL_SIZES.len() as i32) as usize;
        PORTAL_SIZES[idx]
    };

    // Rotation: Util.getRandom(Rotation.values(), random) = nextInt(4)
    let rotation = Rotation::get_random(rng);

    // Mirror: nextFloat() < 0.5 → NONE, else FRONT_BACK
    let mirror_front_back = rng.next_f32() >= 0.5;

    // Compute bounding box via transform with pivot, mirror, and rotation
    // Matches vanilla's template.getBoundingBox(basePosition, rotation, pivot, mirror)
    let pivot_x = sx / 2;
    let pivot_z = sz / 2;
    let bb = rotation.get_bounding_box_full(
        (base_x, 0, base_z),
        (sx, sy, sz),
        pivot_x,
        pivot_z,
        mirror_front_back,
    );
    let bb_min_x = bb.min_x;
    let bb_max_x = bb.max_x;
    let bb_min_z = bb.min_z;
    let bb_max_z = bb.max_z;
    // Vanilla's BoundingBox.getCenter() uses minX + (maxX - minX + 1) / 2,
    // which differs from (minX + maxX) / 2 for even-span BBs due to integer
    // division rounding.
    let bb_center_x = bb_min_x + (bb_max_x - bb_min_x + 1) / 2;
    let bb_center_z = bb_min_z + (bb_max_z - bb_min_z + 1) / 2;

    // Surface height at BB center
    let surface_y = match terrain(TerrainQuery::SurfaceHeight(bb_center_x, bb_center_z)) {
        TerrainResult::Height(h) => h,
        TerrainResult::Opaque(_) => unreachable!(),
    } - 1;

    // findSuitableY — compute newY based on placement type
    let min_y_threshold = min_y + 15;
    let y_span = sy;
    let new_y = match placement {
        Placement::OnLandSurface | Placement::OnOceanFloor => surface_y,
        Placement::Underground => {
            let max_y = surface_y - y_span;
            if min_y_threshold < max_y {
                rng.next_i32_between(min_y_threshold, max_y)
            } else {
                max_y
            }
        }
        Placement::InMountain => {
            let max_y = surface_y - y_span;
            if 70 < max_y {
                rng.next_i32_between(70, max_y)
            } else {
                max_y
            }
        }
        Placement::PartlyBuried => surface_y - y_span + rng.next_i32_between(2, 8),
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

    // findSuitableY corner scan: scan downward from newY, checking if 3 of 4
    // BB corners have an opaque block. Uses WORLD_SURFACE_WG for most placements,
    // OCEAN_FLOOR_WG for ON_OCEAN_FLOOR (but is_opaque handles that externally).
    let min_y_scan = min_y + 15;
    let corners = [
        (bb_min_x, bb_min_z),
        (bb_max_x, bb_min_z),
        (bb_min_x, bb_max_z),
        (bb_max_x, bb_max_z),
    ];

    let mut projected_y = new_y;
    'scan: while projected_y > min_y_scan {
        let mut solid_count = 0;
        for &(cx, cz) in &corners {
            let solid = matches!(
                terrain(TerrainQuery::IsOpaque(cx, projected_y, cz)),
                TerrainResult::Opaque(true)
            );
            if solid {
                solid_count += 1;
                if solid_count == 3 {
                    break 'scan;
                }
            }
        }
        projected_y -= 1;
    }

    // Vanilla's piece BB: template.getBoundingBox(placeSettings, templatePosition)
    // where templatePosition = (base_x, projected_y, base_z).
    let piece_bb = rotation.get_bounding_box_full(
        (base_x, projected_y, base_z),
        (sx, sy, sz),
        pivot_x,
        pivot_z,
        mirror_front_back,
    );

    PortalResult {
        biome_check_pos: (base_x, projected_y, base_z),
        bounding_box: piece_bb,
    }
}

/// `Structure` impl — registered under `"minecraft:ruined_portal"` and its
/// biome variants (desert / jungle / mountain / ocean / swamp / nether).
///
/// The terrain closure creates fresh aquifer + column cache per query because
/// the piece gen may probe positions outside this chunk.
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
                orientation: None,
                nbt_data: Vec::new(),
                ground_level_delta: 0,
                junctions: Vec::new(),
            }],
        })
    }
}
