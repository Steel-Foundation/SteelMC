//! Beardifier: terrain density modification around structure pieces.
//!
//! Matches vanilla's `Beardifier` class. Modifies terrain density at cell corners
//! using a gaussian kernel falloff around rigid structure pieces and jigsaw junctions.
//! This creates the terrain adaptation effects like carving out space for villages
//! or burying ancient cities.

use std::sync::LazyLock;

use steel_registry::structure::TerrainAdjustment;
use steel_registry::template_pool::Projection;
use steel_utils::BoundingBox;
use steel_worldgen::math::map_clamped;

use crate::world::structure::StructureStart;
use crate::world::structure::jigsaw::JigsawJunction;

/// A rigid structure piece that modifies terrain density.
#[derive(Debug)]
struct Rigid {
    bounding_box: BoundingBox,
    terrain_adjustment: TerrainAdjustment,
    ground_level_delta: i32,
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
/// Built per-chunk from the chunk's own starts plus referenced neighbor starts (mirrors
/// vanilla's `StructureManager.startsForStructure`). Queried per-block by `NoiseChunk::fill`
/// after the outer density-function ops, matching vanilla's `cacheAllInCell(add(final_density,
/// beardifier))` integration.
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
    /// `starts` should yield every `StructureStart` whose pieces could affect this chunk —
    /// typically the chunk's own starts plus all referenced neighbor starts (vanilla collects
    /// these via `StructureManager.startsForStructure`).
    ///
    /// `chunk_x` and `chunk_z` are chunk coordinates (not block coordinates).
    ///
    /// Mirrors vanilla's `forStructuresInChunk`:
    /// - Non-jigsaw pieces (`projection: None`) → added as rigid with `ground_level_delta = 0`.
    /// - Jigsaw RIGID pieces → added as rigid with stored `ground_level_delta`, junctions collected.
    /// - Jigsaw `TERRAIN_MATCHING` pieces → not added as rigid; junctions still collected.
    #[must_use]
    pub fn for_structures_in_chunk<'a, I>(starts: I, chunk_x: i32, chunk_z: i32) -> Self
    where
        I: IntoIterator<Item = &'a StructureStart>,
    {
        let chunk_start_x = chunk_x * 16;
        let chunk_start_z = chunk_z * 16;

        let mut rigids = Vec::new();
        let mut junctions: Vec<JigsawJunction> = Vec::new();
        let mut encompassing: Option<BoundingBox> = None;

        for start in starts {
            let terrain_adj = start.terrain_adjustment;
            if terrain_adj == TerrainAdjustment::None {
                continue;
            }

            for piece in &start.pieces {
                let bb = &piece.bounding_box;

                // Vanilla: piece.isCloseToChunk(chunkPos, 12)
                if !is_close_to_chunk(bb, chunk_x, chunk_z, 12) {
                    continue;
                }

                let is_jigsaw = piece.projection.is_some();
                let is_rigid = matches!(piece.projection, Some(Projection::Rigid));

                // Vanilla: only non-jigsaw pieces and jigsaw-RIGID pieces become rigids.
                // Jigsaw TERRAIN_MATCHING pieces are skipped here (junctions still collected
                // below, and the encompassing box only gets junction positions for them).
                if !is_jigsaw || is_rigid {
                    encompassing = Some(match encompassing {
                        Some(enc) => BoundingBox::encapsulating(&enc, bb),
                        None => *bb,
                    });

                    rigids.push(Rigid {
                        bounding_box: *bb,
                        terrain_adjustment: terrain_adj,
                        // Vanilla uses 0 for non-jigsaw pieces regardless of any stored value;
                        // jigsaw RIGID pieces use the projection-derived delta.
                        ground_level_delta: if is_jigsaw {
                            piece.ground_level_delta
                        } else {
                            0
                        },
                    });
                }

                // Junctions: vanilla collects them only on jigsaw pieces, and bounds are
                // strict — exclusive on both sides:
                // `(chunkStartBlockX - 12, chunkStartBlockX + 15 + 12)`, same for Z.
                if is_jigsaw {
                    for junction in &piece.junctions {
                        let jx = junction.source_x;
                        let jz = junction.source_z;
                        if jx > chunk_start_x - 12
                            && jz > chunk_start_z - 12
                            && jx < chunk_start_x + 15 + 12
                            && jz < chunk_start_z + 15 + 12
                        {
                            let jy = junction.source_ground_y;
                            let junction_bb = BoundingBox::new(jx, jy, jz, jx, jy, jz);
                            encompassing = Some(match encompassing {
                                Some(enc) => BoundingBox::encapsulating(&enc, &junction_bb),
                                None => junction_bb,
                            });
                            junctions.push(junction.clone());
                        }
                    }
                }
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
