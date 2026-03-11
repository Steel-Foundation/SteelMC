//! Beardifier: terrain density modification around structure pieces.
//!
//! Matches vanilla's `Beardifier` class. Modifies terrain density at cell corners
//! using a gaussian kernel falloff around rigid structure pieces and jigsaw junctions.
//! This creates the terrain adaptation effects like carving out space for villages
//! or burying ancient cities.

use std::sync::LazyLock;

use steel_utils::math::map_clamped;
use steel_utils::{BoundingBox, Identifier};

use crate::world::structure::StructureStartMap;

/// How a structure modifies the surrounding terrain.
///
/// Corresponds to vanilla's `TerrainAdjustment` enum.
// TODO: This should be data-driven from the structure registry, not hardcoded.
// In vanilla, `TerrainAdjustment` is a codec field on `Structure.StructureSettings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainAdjustment {
    /// No terrain adaptation.
    None,
    /// Fill in terrain around and above the structure (e.g. ancient city).
    Bury,
    /// Carve thin beard below structure (e.g. village).
    BeardThin,
    /// Carve box-shaped beard below structure (e.g. bastion remnant).
    BeardBox,
    /// Encapsulate structure in terrain (e.g. trial chambers).
    Encapsulate,
}

impl TerrainAdjustment {
    /// Look up the terrain adjustment for a vanilla structure identifier.
    // TODO: Replace with registry lookup once structures are data-driven.
    #[must_use]
    pub fn for_structure(id: &Identifier) -> Self {
        if id.namespace != Identifier::VANILLA_NAMESPACE {
            return Self::None;
        }
        match id.path.as_ref() {
            "village" | "pillager_outpost" | "desert_pyramid" | "jungle_temple" | "swamp_hut"
            | "igloo" | "shipwreck" | "shipwreck_beached" | "ocean_ruin_cold"
            | "ocean_ruin_warm" => Self::BeardThin,
            "bastion_remnant" => Self::BeardBox,
            "ancient_city" | "trail_ruins" | "ocean_monument" => Self::Bury,
            "trial_chambers" => Self::Encapsulate,
            _ => Self::None,
        }
    }
}

/// A rigid structure piece that modifies terrain density.
#[derive(Debug)]
struct Rigid {
    bounding_box: BoundingBox,
    terrain_adjustment: TerrainAdjustment,
    ground_level_delta: i32,
}

/// A jigsaw junction point that creates a small terrain beard.
#[derive(Debug)]
pub struct JigsawJunction {
    /// World X coordinate of the junction source.
    pub source_x: i32,
    /// Ground Y level at the junction source.
    pub source_ground_y: i32,
    /// World Z coordinate of the junction source.
    pub source_z: i32,
}

const KERNEL_RADIUS: i32 = 12;
const KERNEL_SIZE: usize = 24;
const KERNEL_TOTAL: usize = KERNEL_SIZE * KERNEL_SIZE * KERNEL_SIZE; // 13824

/// Pre-computed gaussian beard kernel.
/// Layout: `[z][x][y]` where indices go from 0..24, representing offsets -12..+11.
static BEARD_KERNEL: LazyLock<[f32; KERNEL_TOTAL]> = LazyLock::new(|| {
    let mut kernel = [0.0f32; KERNEL_TOTAL];
    for zi in 0..KERNEL_SIZE {
        let dz = zi as i32 - KERNEL_RADIUS;
        for xi in 0..KERNEL_SIZE {
            let dx = xi as i32 - KERNEL_RADIUS;
            for yi in 0..KERNEL_SIZE {
                let dy = yi as i32 - KERNEL_RADIUS;
                // dy + 0.5 matches vanilla's computeBeardContribution(int, int, int)
                let dy_f = f64::from(dy) + 0.5;
                let dist_sq = f64::from(dx * dx) + dy_f * dy_f + f64::from(dz * dz);
                let value = (-dist_sq / 16.0).exp();
                kernel[zi * KERNEL_SIZE * KERNEL_SIZE + xi * KERNEL_SIZE + yi] = value as f32;
            }
        }
    }
    kernel
});

/// Vanilla's `Mth.fastInvSqrt` — the Quake III fast inverse square root, ported exactly.
#[inline]
fn fast_inv_sqrt(x: f64) -> f64 {
    let xhalf = 0.5f64 * x;
    let i = f64::to_bits(x) as i64;
    let i = 0x5FE6_EB50_C7B5_37A9_i64 - (i >> 1);
    let mut x = f64::from_bits(i as u64);
    x *= 1.5f64 - xhalf * x * x;
    x
}

#[inline]
fn is_in_kernel_range(index: i32) -> bool {
    (0..KERNEL_SIZE as i32).contains(&index)
}

/// Computes the beard density contribution for a point near a structure piece.
///
/// `dx`, `dy`, `dz` are the distances from the query point to the piece for kernel lookup.
/// `y_to_ground` is the vertical distance from query point to the piece's ground level.
fn get_beard_contribution(dx: i32, dy: i32, dz: i32, y_to_ground: i32) -> f64 {
    let xi = dx + KERNEL_RADIUS;
    let yi = dy + KERNEL_RADIUS;
    let zi = dz + KERNEL_RADIUS;

    if !is_in_kernel_range(xi) || !is_in_kernel_range(yi) || !is_in_kernel_range(zi) {
        return 0.0;
    }

    let dy_with_offset = f64::from(y_to_ground) + 0.5;
    let dist_sq = f64::from(dx * dx) + dy_with_offset * dy_with_offset + f64::from(dz * dz);
    let value = -dy_with_offset * fast_inv_sqrt(dist_sq / 2.0) / 2.0;
    let kernel_idx =
        zi as usize * KERNEL_SIZE * KERNEL_SIZE + xi as usize * KERNEL_SIZE + yi as usize;
    value * f64::from(BEARD_KERNEL[kernel_idx])
}

/// Computes the bury density contribution for a point near a structure piece.
///
/// Simple linear falloff: 1.0 at distance 0, 0.0 at distance 6.
fn get_bury_contribution(dx: f64, dy: f64, dz: f64) -> f64 {
    let distance = (dx * dx + dy * dy + dz * dz).sqrt();
    map_clamped(distance, 0.0, 6.0, 1.0, 0.0)
}

/// Computes terrain density contributions from nearby structure pieces and junctions.
///
/// Created per-chunk from the chunk's structure starts, then queried at each
/// cell corner during `NoiseChunk::fill_slice`.
pub struct Beardifier {
    rigids: Vec<Rigid>,
    junctions: Vec<JigsawJunction>,
    /// Union of all piece/junction bounding boxes inflated by 24.
    /// Points outside this box get 0.0 without iterating pieces.
    affected_box: Option<BoundingBox>,
}

impl Beardifier {
    /// Collect rigid pieces and junctions from structure starts that affect this chunk.
    ///
    /// `chunk_x` and `chunk_z` are chunk coordinates (not block coordinates).
    #[must_use]
    pub fn for_structures_in_chunk(
        structure_starts: &StructureStartMap,
        chunk_x: i32,
        chunk_z: i32,
    ) -> Self {
        let mut rigids = Vec::new();
        let junctions = Vec::new();
        let mut encompassing: Option<BoundingBox> = None;

        for (structure_id, start) in structure_starts {
            let terrain_adj = TerrainAdjustment::for_structure(structure_id);
            if terrain_adj == TerrainAdjustment::None {
                continue;
            }

            for piece in &start.pieces {
                let bb = &piece.bounding_box;

                // Vanilla: piece.isCloseToChunk(chunkPos, 12)
                // Checks if piece bounding box is within 12 blocks of chunk area
                if !is_close_to_chunk(bb, chunk_x, chunk_z, 12) {
                    continue;
                }

                // For jigsaw pieces with RIGID projection, we'd check projection.
                // Since we store nbt_data opaquely, we treat all pieces as rigid
                // (non-rigid pieces don't generate terrain adaptation in vanilla
                // unless they're PoolElementStructurePiece with RIGID projection).
                // For now, treat all pieces as rigid with ground_level_delta = 0.
                // TODO: Parse ground_level_delta from jigsaw piece NBT when available.
                let ground_level_delta = 0;

                encompassing = Some(match encompassing {
                    Some(enc) => BoundingBox::encapsulating(&enc, bb),
                    None => *bb,
                });

                rigids.push(Rigid {
                    bounding_box: *bb,
                    terrain_adjustment: terrain_adj,
                    ground_level_delta,
                });

                // TODO: Parse and collect jigsaw junctions from piece NBT
                // Junctions contribute getBeardContribution * 0.4
            }
        }

        let affected_box = encompassing
            .map(|bb| bb.inflated_by(KERNEL_SIZE as i32, KERNEL_SIZE as i32, KERNEL_SIZE as i32));

        Self {
            rigids,
            junctions,
            affected_box,
        }
    }

    /// Returns true if there are no pieces or junctions affecting terrain.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.rigids.is_empty() && self.junctions.is_empty()
    }

    /// Compute the total density contribution at a world-space block position.
    ///
    /// Returns 0.0 if no structures are nearby.
    #[must_use]
    pub fn compute(&self, block_x: i32, block_y: i32, block_z: i32) -> f64 {
        let Some(affected) = &self.affected_box else {
            return 0.0;
        };
        if !affected.contains_xyz(block_x, block_y, block_z) {
            return 0.0;
        }

        let mut value = 0.0;

        for rigid in &self.rigids {
            let bb = &rigid.bounding_box;

            // Horizontal distance to closest edge of bounding box (0 if inside)
            let dx = 0.max((bb.min_x - block_x).max(block_x - bb.max_x));
            let dz = 0.max((bb.min_z - block_z).max(block_z - bb.max_z));

            let ground_y = bb.min_y + rigid.ground_level_delta;
            let dy_to_ground = block_y - ground_y;

            match rigid.terrain_adjustment {
                TerrainAdjustment::None => {}
                TerrainAdjustment::Bury => {
                    value += get_bury_contribution(
                        f64::from(dx),
                        f64::from(dy_to_ground) / 2.0,
                        f64::from(dz),
                    );
                }
                TerrainAdjustment::BeardThin => {
                    value += get_beard_contribution(dx, dy_to_ground, dz, dy_to_ground) * 0.8;
                }
                TerrainAdjustment::BeardBox => {
                    let dy = 0.max((ground_y - block_y).max(block_y - bb.max_y));
                    value += get_beard_contribution(dx, dy, dz, dy_to_ground) * 0.8;
                }
                TerrainAdjustment::Encapsulate => {
                    let dy = 0.max((bb.min_y - block_y).max(block_y - bb.max_y));
                    value += get_bury_contribution(
                        f64::from(dx) / 2.0,
                        f64::from(dy) / 2.0,
                        f64::from(dz) / 2.0,
                    ) * 0.8;
                }
            }
        }

        for junction in &self.junctions {
            let dx = block_x - junction.source_x;
            let dy = block_y - junction.source_ground_y;
            let dz = block_z - junction.source_z;
            value += get_beard_contribution(dx, dy, dz, dy) * 0.4;
        }

        value
    }
}

/// Check if a bounding box is within `margin` blocks of a chunk.
///
/// Matches vanilla's `StructurePiece.isCloseToChunk(ChunkPos, int)`.
const fn is_close_to_chunk(bb: &BoundingBox, chunk_x: i32, chunk_z: i32, margin: i32) -> bool {
    let chunk_start_x = chunk_x * 16;
    let chunk_start_z = chunk_z * 16;
    let chunk_end_x = chunk_start_x + 15;
    let chunk_end_z = chunk_start_z + 15;

    bb.max_x >= chunk_start_x - margin
        && bb.min_x <= chunk_end_x + margin
        && bb.max_z >= chunk_start_z - margin
        && bb.min_z <= chunk_end_z + margin
}
