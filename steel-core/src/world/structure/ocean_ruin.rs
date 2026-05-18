//! Ocean ruin: a base piece from a warm/cold × small/large pool, plus — when large
//! and the cluster check passes — a scatter of smaller ruins with collision checks.
//! Warm uses one piece; cold stacks three (brick + cracked + mossy) from the same index.

use steel_registry::structure::{OceanRuinBiomeTempData, StructureConfigData, StructureData};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Direction, Identifier, Rotation};

use crate::world::structure::{
    GenerationStub, Structure, StructureGenerationContext, StructurePiece,
};

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

fn template_bb(
    ctx: &dyn StructureGenerationContext,
    name: &str,
    px: i32,
    pz: i32,
    rot: Rotation,
) -> Option<BoundingBox> {
    let key = Identifier::new("minecraft", name.to_string());
    ctx.templates()
        .get(&key)
        .map(|t| rot.get_bounding_box(px, 90, pz, t.size[0], t.size[1], t.size[2]))
}

/// `(x_base, z_base, x_between, z_between)` for a single candidate.
type ClusterOffset = (i32, i32, (i32, i32), (i32, i32));

/// Vanilla's 8 candidate offsets around a parent ruin.
#[rustfmt::skip]
const CLUSTER_OFFSETS: [ClusterOffset; 8] = [
    (-16,  16, (1, 8), (1, 7)),
    (-16,   0, (1, 8), (1, 7)),
    (-16, -16, (1, 8), (4, 8)),
    (  0,  16, (1, 7), (1, 7)),
    (  0, -16, (1, 7), (4, 6)),
    ( 16,  16, (1, 7), (3, 8)),
    ( 16,   0, (1, 7), (1, 7)),
    ( 16, -16, (1, 7), (4, 8)),
];

const fn ocean_ruin_piece(bb: BoundingBox) -> StructurePiece {
    StructurePiece::non_jigsaw(
        Identifier::new_static("minecraft", "orp"),
        bb,
        0,
        Some(Direction::North),
    )
}

/// Registered under `"minecraft:ocean_ruin"`. Warm/cold are distinguished by
/// `entry.structure.path`.
pub struct OceanRuinStructure;

impl Structure for OceanRuinStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let ocean_floor_y = ctx.base_height(ctx.center_block_x(), ctx.center_block_z(), true) - 1;
        let biome = ctx.biome_at(ctx.center_block_x(), ocean_floor_y, ctx.center_block_z());
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let StructureConfigData::OceanRuin {
            biome_temp,
            large_probability,
            cluster_probability,
        } = &structure.config
        else {
            return None;
        };
        let is_warm = matches!(biome_temp, OceanRuinBiomeTempData::Warm);
        let rotation = Rotation::get_random(rng);
        let is_large = rng.next_f32() <= *large_probability;
        let (pos_x, pos_z) = (ctx.chunk_min_x(), ctx.chunk_min_z());

        let mut bbs: Vec<BoundingBox> = Vec::new();
        let push_bb = |bbs: &mut Vec<BoundingBox>, name: &str, x, z, rot| {
            if let Some(bb) = template_bb(ctx, name, x, z, rot) {
                bbs.push(bb);
            }
        };

        if is_warm {
            let arr = if is_large { WARM_LARGE } else { WARM_SMALL };
            let idx = rng.next_i32_bounded(arr.len() as i32) as usize;
            push_bb(&mut bbs, arr[idx], pos_x, pos_z, rotation);
        } else {
            let (bricks, cracked, mossy) = if is_large {
                (COLD_BIG_BRICK, COLD_BIG_CRACKED, COLD_BIG_MOSSY)
            } else {
                (COLD_BRICK, COLD_CRACKED, COLD_MOSSY)
            };
            let idx = rng.next_i32_bounded(bricks.len() as i32) as usize;
            push_bb(&mut bbs, bricks[idx], pos_x, pos_z, rotation);
            push_bb(&mut bbs, cracked[idx], pos_x, pos_z, rotation);
            push_bb(&mut bbs, mossy[idx], pos_x, pos_z, rotation);
        }

        if is_large && rng.next_f32() <= *cluster_probability {
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
            let bl_x = pos_x.min(parent_corner_x);
            let bl_z = pos_z.min(parent_corner_z);

            let mut candidates: Vec<(i32, i32)> = CLUSTER_OFFSETS
                .iter()
                .map(|&(ox, oz, (xa, xb), (za, zb))| {
                    (
                        bl_x + ox + rng.next_i32_between(xa, xb),
                        bl_z + oz + rng.next_i32_between(za, zb),
                    )
                })
                .collect();

            for _ in 0..rng.next_i32_between(4, 8) {
                if candidates.is_empty() {
                    break;
                }
                let idx = rng.next_i32_bounded(candidates.len() as i32) as usize;
                let (cx, cz) = candidates.remove(idx);
                let cluster_rot = Rotation::get_random(rng);
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
                    if is_warm {
                        let tidx = rng.next_i32_bounded(WARM_SMALL.len() as i32) as usize;
                        push_bb(&mut bbs, WARM_SMALL[tidx], cx, cz, cluster_rot);
                    } else {
                        let tidx = rng.next_i32_bounded(COLD_BRICK.len() as i32) as usize;
                        push_bb(&mut bbs, COLD_BRICK[tidx], cx, cz, cluster_rot);
                        push_bb(&mut bbs, COLD_CRACKED[tidx], cx, cz, cluster_rot);
                        push_bb(&mut bbs, COLD_MOSSY[tidx], cx, cz, cluster_rot);
                    }
                }
            }
        }

        Some(GenerationStub {
            position: (ctx.center_block_x(), ocean_floor_y, ctx.center_block_z()),
            pieces: bbs.into_iter().map(ocean_ruin_piece).collect(),
        })
    }
}
