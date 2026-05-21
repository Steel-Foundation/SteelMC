//! Igloo: one top piece always, 50% chance of a basement (laboratory + `depth-1`
//! ladder segments, depth ∈ [4, 11]).

use steel_registry::structure::{LiquidSettingsData, StructureData};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{Direction, Identifier, Rotation};

use crate::world::structure::{
    GenerationStub, Structure, StructureBlockIgnore, StructureGenerationContext, StructureMirror,
    StructurePiece, StructurePiecePayload, TemplateMarkerHandling, TemplatePieceData,
    TemplatePlacementAdjustment, TemplatePlacementClip, TemplatePostProcess, TemplateProcessorList,
};

const TOP_SIZE: [i32; 3] = [7, 5, 8];
const MID_SIZE: [i32; 3] = [3, 3, 3];
const BOT_SIZE: [i32; 3] = [7, 6, 9];
const TOP_PIVOT: (i32, i32, i32) = (3, 5, 5);
const MID_PIVOT: (i32, i32, i32) = (1, 3, 1);
const BOT_PIVOT: (i32, i32, i32) = (3, 6, 7);
const TOP_OFF: (i32, i32, i32) = (0, 0, 0);
const MID_OFF: (i32, i32, i32) = (2, -3, 4);
const BOT_OFF: (i32, i32, i32) = (0, -3, -2);
const GEN_Y: i32 = 90;

#[expect(
    clippy::too_many_arguments,
    reason = "igloo piece construction mirrors vanilla template-piece constants"
)]
const fn make_igloo_piece(
    template_path: &'static str,
    start_x: i32,
    start_z: i32,
    rotation: Rotation,
    off: (i32, i32, i32),
    depth: i32,
    size: [i32; 3],
    pivot: (i32, i32, i32),
    post_process: TemplatePostProcess,
) -> StructurePiece {
    let template_position = (start_x + off.0, GEN_Y + off.1 - depth, start_z + off.2);
    StructurePiece {
        piece_type: Identifier::new_static("minecraft", "iglu"),
        bounding_box: rotation.get_bounding_box_with_pivot(
            template_position,
            (size[0], size[1], size[2]),
            pivot.0,
            pivot.2,
        ),
        gen_depth: 0,
        orientation: Some(Direction::North),
        payload: StructurePiecePayload::Template(TemplatePieceData {
            template_id: Identifier::vanilla_static(template_path),
            template_position,
            rotation,
            mirror: StructureMirror::None,
            rotation_pivot: pivot,
            block_ignore: StructureBlockIgnore::StructureBlock,
            late_block_ignore: StructureBlockIgnore::None,
            processors: TemplateProcessorList::Empty,
            liquid_settings: LiquidSettingsData::IgnoreWaterlogging,
            marker_handling: TemplateMarkerHandling::Igloo,
            placement_adjustment: TemplatePlacementAdjustment::Igloo {
                template_offset: off,
            },
            placement_clip: TemplatePlacementClip::CenterChunk,
            post_process,
        }),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    }
}

/// Registered under `"minecraft:igloo"`.
pub struct IglooStructure;

impl Structure for IglooStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let surface_y = ctx.surface_y();
        let biome = ctx.biome_at(ctx.center_block_x(), surface_y, ctx.center_block_z());
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let rotation = Rotation::get_random(rng);
        let (start_x, start_z) = (ctx.chunk_min_x(), ctx.chunk_min_z());

        let mut pieces = Vec::new();
        if rng.next_f64() < 0.5_f64 {
            let depth = rng.next_i32_bounded(8) + 4;
            pieces.push(make_igloo_piece(
                "igloo/bottom",
                start_x,
                start_z,
                rotation,
                BOT_OFF,
                depth * 3,
                BOT_SIZE,
                BOT_PIVOT,
                TemplatePostProcess::None,
            ));
            for i in 0..depth - 1 {
                pieces.push(make_igloo_piece(
                    "igloo/middle",
                    start_x,
                    start_z,
                    rotation,
                    MID_OFF,
                    i * 3,
                    MID_SIZE,
                    MID_PIVOT,
                    TemplatePostProcess::None,
                ));
            }
        }
        pieces.push(make_igloo_piece(
            "igloo/top",
            start_x,
            start_z,
            rotation,
            TOP_OFF,
            0,
            TOP_SIZE,
            TOP_PIVOT,
            TemplatePostProcess::IglooTop,
        ));

        Some(GenerationStub {
            position: (ctx.center_block_x(), surface_y, ctx.center_block_z()),
            pieces,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn igloo_piece_uses_template_payload_with_height_adjustment() {
        let piece = make_igloo_piece(
            "igloo/top",
            32,
            -48,
            Rotation::Clockwise90,
            TOP_OFF,
            0,
            TOP_SIZE,
            TOP_PIVOT,
            TemplatePostProcess::IglooTop,
        );

        assert_eq!(
            piece.piece_type,
            Identifier::new_static("minecraft", "iglu")
        );
        assert_eq!(piece.gen_depth, 0);
        assert_eq!(piece.orientation, Some(Direction::North));
        assert_eq!(
            piece.bounding_box,
            Rotation::Clockwise90.get_bounding_box_with_pivot(
                (32, GEN_Y, -48),
                (TOP_SIZE[0], TOP_SIZE[1], TOP_SIZE[2]),
                TOP_PIVOT.0,
                TOP_PIVOT.2,
            ),
        );

        let StructurePiecePayload::Template(data) = piece.payload else {
            panic!("igloo piece should be template-backed");
        };
        assert_eq!(data.template_id, Identifier::vanilla_static("igloo/top"));
        assert_eq!(data.template_position, (32, GEN_Y, -48));
        assert_eq!(data.rotation, Rotation::Clockwise90);
        assert_eq!(data.mirror, StructureMirror::None);
        assert_eq!(data.rotation_pivot, TOP_PIVOT);
        assert_eq!(data.block_ignore, StructureBlockIgnore::StructureBlock);
        assert_eq!(data.late_block_ignore, StructureBlockIgnore::None);
        assert_eq!(data.processors, TemplateProcessorList::Empty);
        assert_eq!(data.liquid_settings, LiquidSettingsData::IgnoreWaterlogging);
        assert_eq!(data.marker_handling, TemplateMarkerHandling::Igloo);
        assert_eq!(
            data.placement_adjustment,
            TemplatePlacementAdjustment::Igloo {
                template_offset: TOP_OFF,
            }
        );
        assert_eq!(data.placement_clip, TemplatePlacementClip::CenterChunk);
        assert_eq!(data.post_process, TemplatePostProcess::IglooTop);
    }
}
