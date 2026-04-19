//! Ocean ruin piece generation.
//!
//! Vanilla `OceanRuinPieces.addPieces`: a base piece chosen from a warm/cold +
//! small/large template pool, plus (when large AND a cluster check passes) a
//! scatter of smaller ruins around the parent with collision checking.
//!
//! Warm → one piece. Cold → three stacked pieces (brick + cracked + mossy) at
//! the same position, all from the same index.

use steel_utils::density::DimensionNoises;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Identifier, Rotation};

use crate::world::structure::placement::StructureSelectionEntry;
use crate::world::structure::{GenerationContext, GenerationStub, Structure, StructurePiece};

const LARGE_PROB: f32 = 0.3;
const CLUSTER_PROB: f32 = 0.9;

static WARM_SMALL: &[&str] = &[
    "underwater_ruin/warm_1",
    "underwater_ruin/warm_2",
    "underwater_ruin/warm_3",
    "underwater_ruin/warm_4",
    "underwater_ruin/warm_5",
    "underwater_ruin/warm_6",
    "underwater_ruin/warm_7",
    "underwater_ruin/warm_8",
];
static WARM_LARGE: &[&str] = &[
    "underwater_ruin/big_warm_4",
    "underwater_ruin/big_warm_5",
    "underwater_ruin/big_warm_6",
    "underwater_ruin/big_warm_7",
];
static COLD_BRICK: &[&str] = &[
    "underwater_ruin/brick_1",
    "underwater_ruin/brick_2",
    "underwater_ruin/brick_3",
    "underwater_ruin/brick_4",
    "underwater_ruin/brick_5",
    "underwater_ruin/brick_6",
    "underwater_ruin/brick_7",
    "underwater_ruin/brick_8",
];
static COLD_CRACKED: &[&str] = &[
    "underwater_ruin/cracked_1",
    "underwater_ruin/cracked_2",
    "underwater_ruin/cracked_3",
    "underwater_ruin/cracked_4",
    "underwater_ruin/cracked_5",
    "underwater_ruin/cracked_6",
    "underwater_ruin/cracked_7",
    "underwater_ruin/cracked_8",
];
static COLD_MOSSY: &[&str] = &[
    "underwater_ruin/mossy_1",
    "underwater_ruin/mossy_2",
    "underwater_ruin/mossy_3",
    "underwater_ruin/mossy_4",
    "underwater_ruin/mossy_5",
    "underwater_ruin/mossy_6",
    "underwater_ruin/mossy_7",
    "underwater_ruin/mossy_8",
];
static COLD_BIG_BRICK: &[&str] = &[
    "underwater_ruin/big_brick_1",
    "underwater_ruin/big_brick_2",
    "underwater_ruin/big_brick_3",
    "underwater_ruin/big_brick_8",
];
static COLD_BIG_CRACKED: &[&str] = &[
    "underwater_ruin/big_cracked_1",
    "underwater_ruin/big_cracked_2",
    "underwater_ruin/big_cracked_3",
    "underwater_ruin/big_cracked_8",
];
static COLD_BIG_MOSSY: &[&str] = &[
    "underwater_ruin/big_mossy_1",
    "underwater_ruin/big_mossy_2",
    "underwater_ruin/big_mossy_3",
    "underwater_ruin/big_mossy_8",
];

fn template_bb<N: DimensionNoises>(
    ctx: &GenerationContext<'_, '_, N>,
    name: &str,
    px: i32,
    pz: i32,
    rot: Rotation,
) -> Option<BoundingBox> {
    let key = Identifier::new("minecraft", name.to_string());
    ctx.templates
        .get(&key)
        .map(|t| rot.get_bounding_box(px, 90, pz, t.size[0], t.size[1], t.size[2]))
}

/// `Structure` impl registered under `"minecraft:ocean_ruin"`. Warm/cold
/// variants are distinguished via `entry.structure.path`.
pub struct OceanRuinStructure;

impl<N: DimensionNoises> Structure<N> for OceanRuinStructure {
    #[expect(
        clippy::too_many_lines,
        reason = "mirrors vanilla's OceanRuinStructure cluster-scatter piece emission"
    )]
    fn find_generation_point(
        &self,
        ctx: &mut GenerationContext<'_, '_, N>,
        entry: &StructureSelectionEntry,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        // Biome check at chunk center, ocean-floor Y.
        let ocean_floor_y = ctx.base_height(ctx.center_block_x, ctx.center_block_z, true) - 1;
        let biome = ctx.biome_at(ctx.center_block_x, ocean_floor_y, ctx.center_block_z);
        if !entry.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let is_warm = entry.structure.path.contains("warm");
        let rotation = Rotation::get_random(rng);
        let is_large = rng.next_f32() <= LARGE_PROB;

        let mut bbs: Vec<BoundingBox> = Vec::new();
        let pos_x = ctx.chunk_min_x;
        let pos_z = ctx.chunk_min_z;

        if is_warm {
            let arr = if is_large { WARM_LARGE } else { WARM_SMALL };
            let idx = rng.next_i32_bounded(arr.len() as i32) as usize;
            if let Some(bb) = template_bb(ctx, arr[idx], pos_x, pos_z, rotation) {
                bbs.push(bb);
            }
        } else {
            let bricks = if is_large { COLD_BIG_BRICK } else { COLD_BRICK };
            let cracked = if is_large {
                COLD_BIG_CRACKED
            } else {
                COLD_CRACKED
            };
            let mossy = if is_large { COLD_BIG_MOSSY } else { COLD_MOSSY };
            let idx = rng.next_i32_bounded(bricks.len() as i32) as usize;
            if let Some(bb) = template_bb(ctx, bricks[idx], pos_x, pos_z, rotation) {
                bbs.push(bb);
            }
            if let Some(bb) = template_bb(ctx, cracked[idx], pos_x, pos_z, rotation) {
                bbs.push(bb);
            }
            if let Some(bb) = template_bb(ctx, mossy[idx], pos_x, pos_z, rotation) {
                bbs.push(bb);
            }
        }

        // Cluster scatter (only if large + cluster check passes).
        if is_large && rng.next_f32() <= CLUSTER_PROB {
            let (pc_x, _, pc_z) = rotation.transform_pos(15, 0, 15, 0, 0);
            let parent_corner_x = pos_x + pc_x;
            let parent_corner_z = pos_z + pc_z;
            let parent_bb = BoundingBox::new(
                pos_x.min(parent_corner_x),
                0,
                pos_z.min(parent_corner_z),
                pos_x.max(parent_corner_x),
                255,
                pos_z.max(parent_corner_z),
            );
            let bottom_left_x = pos_x.min(parent_corner_x);
            let bottom_left_z = pos_z.min(parent_corner_z);

            // 8 candidate positions around the parent.
            let mut candidates = Vec::with_capacity(8);
            candidates.push((
                bottom_left_x - 16 + rng.next_i32_between(1, 8),
                bottom_left_z + 16 + rng.next_i32_between(1, 7),
            ));
            candidates.push((
                bottom_left_x - 16 + rng.next_i32_between(1, 8),
                bottom_left_z + rng.next_i32_between(1, 7),
            ));
            candidates.push((
                bottom_left_x - 16 + rng.next_i32_between(1, 8),
                bottom_left_z - 16 + rng.next_i32_between(4, 8),
            ));
            candidates.push((
                bottom_left_x + rng.next_i32_between(1, 7),
                bottom_left_z + 16 + rng.next_i32_between(1, 7),
            ));
            candidates.push((
                bottom_left_x + rng.next_i32_between(1, 7),
                bottom_left_z - 16 + rng.next_i32_between(4, 6),
            ));
            candidates.push((
                bottom_left_x + 16 + rng.next_i32_between(1, 7),
                bottom_left_z + 16 + rng.next_i32_between(3, 8),
            ));
            candidates.push((
                bottom_left_x + 16 + rng.next_i32_between(1, 7),
                bottom_left_z + rng.next_i32_between(1, 7),
            ));
            candidates.push((
                bottom_left_x + 16 + rng.next_i32_between(1, 7),
                bottom_left_z - 16 + rng.next_i32_between(4, 8),
            ));

            let ruins_count = rng.next_i32_between(4, 8);
            for _ in 0..ruins_count {
                if candidates.is_empty() {
                    break;
                }
                let idx = rng.next_i32_bounded(candidates.len() as i32) as usize;
                let (cx, cz) = candidates.remove(idx);
                let cluster_rot = Rotation::get_random(rng);
                // Collision check with parent BB.
                let (nc_x, _, nc_z) = cluster_rot.transform_pos(5, 0, 6, 0, 0);
                let cluster_bb = BoundingBox::new(
                    cx.min(cx + nc_x),
                    0,
                    cz.min(cz + nc_z),
                    cx.max(cx + nc_x),
                    255,
                    cz.max(cz + nc_z),
                );
                if !cluster_bb.intersects(&parent_bb) {
                    let cluster_arr = if is_warm { WARM_SMALL } else { COLD_BRICK };
                    let tidx = rng.next_i32_bounded(cluster_arr.len() as i32) as usize;
                    if let Some(bb) = template_bb(ctx, cluster_arr[tidx], cx, cz, cluster_rot) {
                        bbs.push(bb);
                    }
                }
            }
        }

        let pieces = bbs
            .into_iter()
            .map(|bb| StructurePiece {
                piece_type: Identifier::new_static("minecraft", "orp"),
                bounding_box: bb,
                gen_depth: 0,
                orientation: None,
                nbt_data: Vec::new(),
                ground_level_delta: 0,
                junctions: Vec::new(),
            })
            .collect();

        Some(GenerationStub {
            position: (ctx.center_block_x, ocean_floor_y, ctx.center_block_z),
            pieces,
        })
    }
}
