//! Ocean ruin: a base piece from a warm/cold × small/large pool, plus — when large
//! and the cluster check passes — a scatter of smaller ruins with collision checks.
//! Warm uses one piece; cold stacks three (brick + cracked + mossy) from the same index.

use steel_registry::structure::{
    LiquidSettingsData, OceanRuinBiomeTempData, StructureConfigData, StructureData,
};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Direction, Identifier, Rotation};

use crate::structure::{
    GenerationStub, Structure, StructureBlockIgnore, StructureGenerationContext, StructureMirror,
    StructurePiece, StructurePiecePayload, TemplateMarkerHandling, TemplatePieceData,
    TemplatePlacementAdjustment, TemplatePlacementClip, TemplatePostProcess, TemplateProcessorList,
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

const fn template_bb(position: (i32, i32, i32), size: [i32; 3], rotation: Rotation) -> BoundingBox {
    rotation.get_bounding_box(
        position.0, position.1, position.2, size[0], size[1], size[2],
    )
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

const fn ocean_ruin_piece(
    template_id: Identifier,
    position: (i32, i32, i32),
    size: [i32; 3],
    rotation: Rotation,
    biome_temp: OceanRuinBiomeTempData,
    is_large: bool,
    integrity: f32,
) -> StructurePiece {
    StructurePiece {
        piece_type: Identifier::new_static("minecraft", "orp"),
        bounding_box: template_bb(position, size, rotation),
        gen_depth: 0,
        orientation: Some(Direction::North),
        payload: StructurePiecePayload::Template(TemplatePieceData {
            template_id,
            template_position: position,
            rotation,
            mirror: StructureMirror::None,
            rotation_pivot: (0, 0, 0),
            block_ignore: StructureBlockIgnore::None,
            late_block_ignore: StructureBlockIgnore::StructureAndAir,
            processors: TemplateProcessorList::OceanRuin {
                biome_temp,
                integrity,
            },
            liquid_settings: LiquidSettingsData::ApplyWaterlogging,
            marker_handling: TemplateMarkerHandling::OceanRuin { is_large },
            placement_adjustment: TemplatePlacementAdjustment::OceanRuin,
            placement_clip: TemplatePlacementClip::CenterChunk,
            post_process: TemplatePostProcess::None,
        }),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    }
}

/// Registered under `"minecraft:ocean_ruin"`. Warm/cold are distinguished by
/// `entry.structure.path`.
pub struct OceanRuinStructure;

impl Structure for OceanRuinStructure {
    #[expect(
        clippy::too_many_lines,
        reason = "keeps vanilla's warm/cold large-ruin cluster generation in one RNG-ordered flow"
    )]
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

        let mut pieces: Vec<StructurePiece> = Vec::new();
        let push_piece =
            |pieces: &mut Vec<StructurePiece>, name: &str, x, z, rot, is_large_piece, integrity| {
                let template_id = Identifier::new("minecraft", name.to_string());
                if let Some(template) = ctx.templates().get(&template_id) {
                    pieces.push(ocean_ruin_piece(
                        template_id,
                        (x, 90, z),
                        template.size,
                        rot,
                        *biome_temp,
                        is_large_piece,
                        integrity,
                    ));
                }
            };
        let base_integrity = if is_large { 0.9 } else { 0.8 };

        if is_warm {
            let arr = if is_large { WARM_LARGE } else { WARM_SMALL };
            let idx = rng.next_i32_bounded(arr.len() as i32) as usize;
            push_piece(
                &mut pieces,
                arr[idx],
                pos_x,
                pos_z,
                rotation,
                is_large,
                base_integrity,
            );
        } else {
            let (bricks, cracked, mossy) = if is_large {
                (COLD_BIG_BRICK, COLD_BIG_CRACKED, COLD_BIG_MOSSY)
            } else {
                (COLD_BRICK, COLD_CRACKED, COLD_MOSSY)
            };
            let idx = rng.next_i32_bounded(bricks.len() as i32) as usize;
            push_piece(
                &mut pieces,
                bricks[idx],
                pos_x,
                pos_z,
                rotation,
                is_large,
                base_integrity,
            );
            push_piece(
                &mut pieces,
                cracked[idx],
                pos_x,
                pos_z,
                rotation,
                is_large,
                0.7,
            );
            push_piece(
                &mut pieces,
                mossy[idx],
                pos_x,
                pos_z,
                rotation,
                is_large,
                0.5,
            );
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
                        push_piece(
                            &mut pieces,
                            WARM_SMALL[tidx],
                            cx,
                            cz,
                            cluster_rot,
                            false,
                            0.8,
                        );
                    } else {
                        let tidx = rng.next_i32_bounded(COLD_BRICK.len() as i32) as usize;
                        push_piece(
                            &mut pieces,
                            COLD_BRICK[tidx],
                            cx,
                            cz,
                            cluster_rot,
                            false,
                            0.8,
                        );
                        push_piece(
                            &mut pieces,
                            COLD_CRACKED[tidx],
                            cx,
                            cz,
                            cluster_rot,
                            false,
                            0.7,
                        );
                        push_piece(
                            &mut pieces,
                            COLD_MOSSY[tidx],
                            cx,
                            cz,
                            cluster_rot,
                            false,
                            0.5,
                        );
                    }
                }
            }
        }

        Some(GenerationStub {
            position: (ctx.center_block_x(), ocean_floor_y, ctx.center_block_z()),
            pieces,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocean_ruin_piece_uses_template_payload_with_height_adjustment_and_processors() {
        let template_id = Identifier::vanilla_static("underwater_ruin/warm_1");
        let position = (32, 90, -48);
        let size = [9, 7, 9];
        let piece = ocean_ruin_piece(
            template_id.clone(),
            position,
            size,
            Rotation::Clockwise90,
            OceanRuinBiomeTempData::Warm,
            false,
            0.8,
        );

        assert_eq!(piece.piece_type, Identifier::new_static("minecraft", "orp"));
        assert_eq!(piece.gen_depth, 0);
        assert_eq!(piece.orientation, Some(Direction::North));
        assert_eq!(
            piece.bounding_box,
            Rotation::Clockwise90.get_bounding_box(
                position.0, position.1, position.2, size[0], size[1], size[2],
            )
        );

        let StructurePiecePayload::Template(data) = piece.payload else {
            panic!("ocean ruin piece should be template-backed");
        };
        assert_eq!(data.template_id, template_id);
        assert_eq!(data.template_position, position);
        assert_eq!(data.rotation, Rotation::Clockwise90);
        assert_eq!(data.mirror, StructureMirror::None);
        assert_eq!(data.rotation_pivot, (0, 0, 0));
        assert_eq!(data.block_ignore, StructureBlockIgnore::None);
        assert_eq!(
            data.late_block_ignore,
            StructureBlockIgnore::StructureAndAir
        );
        assert_eq!(
            data.processors,
            TemplateProcessorList::OceanRuin {
                biome_temp: OceanRuinBiomeTempData::Warm,
                integrity: 0.8,
            }
        );
        assert_eq!(data.liquid_settings, LiquidSettingsData::ApplyWaterlogging);
        assert_eq!(
            data.marker_handling,
            TemplateMarkerHandling::OceanRuin { is_large: false }
        );
        assert_eq!(
            data.placement_adjustment,
            TemplatePlacementAdjustment::OceanRuin
        );
        assert_eq!(data.placement_clip, TemplatePlacementClip::CenterChunk);
        assert_eq!(data.post_process, TemplatePostProcess::None);
    }
}
