//! Ruined portal biome check position computation.
//!
//! Mirrors vanilla's `RuinedPortalStructure.findGenerationPoint` RNG consumption
//! to determine the correct Y for the biome check. Does not place actual blocks.

use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Rotation};

/// Template sizes (x, y, z) for regular portals (portal_1 through portal_10).
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

/// Template sizes (x, y, z) for giant portals (giant_portal_1 through giant_portal_3).
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
            Setup { placement: Placement::Underground, weight: 0.5, air_pocket_prob: 1.0 },
            Setup { placement: Placement::OnLandSurface, weight: 0.5, air_pocket_prob: 0.5 },
        ],
        "ruined_portal_desert" => vec![
            Setup { placement: Placement::PartlyBuried, weight: 1.0, air_pocket_prob: 0.0 },
        ],
        "ruined_portal_jungle" => vec![
            Setup { placement: Placement::OnLandSurface, weight: 1.0, air_pocket_prob: 0.5 },
        ],
        "ruined_portal_mountain" => vec![
            Setup { placement: Placement::InMountain, weight: 0.5, air_pocket_prob: 1.0 },
            Setup { placement: Placement::OnLandSurface, weight: 0.5, air_pocket_prob: 0.5 },
        ],
        "ruined_portal_ocean" => vec![
            Setup { placement: Placement::OnOceanFloor, weight: 1.0, air_pocket_prob: 0.0 },
        ],
        "ruined_portal_swamp" => vec![
            Setup { placement: Placement::OnOceanFloor, weight: 1.0, air_pocket_prob: 0.0 },
        ],
        "ruined_portal_nether" => vec![
            Setup { placement: Placement::InNether, weight: 1.0, air_pocket_prob: 0.5 },
        ],
        _ => vec![
            Setup { placement: Placement::OnLandSurface, weight: 1.0, air_pocket_prob: 0.0 },
        ],
    }
}

/// Applies vanilla's `StructureTemplate.transform` for a position relative to a pivot.
///
/// `rotation`: 0=NONE, 1=CW_90, 2=CW_180, 3=CCW_90.
/// `mirror_front_back`: true for FRONT_BACK mirror.
fn transform(x: i32, z: i32, pivot_x: i32, pivot_z: i32, rotation: i32, mirror_front_back: bool) -> (i32, i32) {
    let mut dx = x - pivot_x;
    let dz = z - pivot_z;

    // Mirror FRONT_BACK flips X
    if mirror_front_back {
        dx = -dx;
    }

    let (rx, rz) = match rotation {
        0 => (dx, dz),
        1 => (-dz, dx),  // CW_90
        2 => (-dx, -dz), // CW_180
        3 => (dz, -dx),  // CCW_90
        _ => (dx, dz),
    };

    (rx + pivot_x, rz + pivot_z)
}

/// Terrain query operations needed by the ruined portal generation.
pub enum TerrainQuery {
    /// Get surface height at (x, z). Returns first solid Y from top.
    SurfaceHeight(i32, i32),
    /// Check if block at (x, y, z) is opaque for WORLD_SURFACE_WG heightmap.
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
    let rotation = rng.next_i32_bounded(4);

    // Mirror: nextFloat() < 0.5 → FRONT_BACK, else NONE
    let mirror_front_back = rng.next_f32() < 0.5;

    // Compute bounding box via transform of (size-1) around pivot
    let pivot_x = sx / 2;
    let pivot_z = sz / 2;
    let (tx, tz) = transform(sx - 1, sz - 1, pivot_x, pivot_z, rotation, mirror_front_back);

    // BB corners: base and base+transformed
    let bb_min_x = base_x.min(base_x + tx);
    let bb_max_x = base_x.max(base_x + tx);
    let bb_min_z = base_z.min(base_z + tz);
    let bb_max_z = base_z.max(base_z + tz);
    let bb_center_x = (bb_min_x + bb_max_x) / 2;
    let bb_center_z = (bb_min_z + bb_max_z) / 2;

    // Surface height at BB center
    let surface_y = match terrain(TerrainQuery::SurfaceHeight(bb_center_x, bb_center_z)) {
        TerrainResult::Height(h) => h,
        _ => unreachable!(),
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
        Placement::PartlyBuried => {
            surface_y - y_span + rng.next_i32_between(2, 8)
        }
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
            let solid = matches!(terrain(TerrainQuery::IsOpaque(cx, projected_y, cz)), TerrainResult::Opaque(true));
            if solid {
                solid_count += 1;
                if solid_count == 3 {
                    break 'scan;
                }
            }
        }
        projected_y -= 1;
    }

    // Vanilla's piece BB: template at (base_x, projected_y, base_z) with rotation.
    // The XZ extent was already computed above (bb_min_x..bb_max_x, bb_min_z..bb_max_z).
    // Y range: projected_y to projected_y + template_height - 1.
    let piece_bb = BoundingBox::new(
        bb_min_x, projected_y, bb_min_z,
        bb_max_x, projected_y + sy - 1, bb_max_z,
    );

    PortalResult {
        biome_check_pos: (base_x, projected_y, base_z),
        bounding_box: piece_bb,
    }
}
