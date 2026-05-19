//! Shipwreck: picks a random template from the beached (11) or underwater (20) pool,
//! places at `(chunkMinX, 90, chunkMinZ)` with random rotation and pivot `(4, 15)`.

use steel_registry::structure::{LiquidSettingsData, StructureConfigData, StructureData};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{Direction, Identifier, Rotation};

use crate::world::structure::{
    GenerationStub, Structure, StructureBlockIgnore, StructureGenerationContext, StructureMirror,
    StructurePiece, StructurePiecePayload, TemplateMarkerHandling, TemplatePieceData,
    TemplatePlacementAdjustment, TemplatePlacementClip, TemplatePostProcess, TemplateProcessorList,
};

static BEACHED: &[&str] = &[
    "shipwreck/with_mast",
    "shipwreck/sideways_full",
    "shipwreck/sideways_fronthalf",
    "shipwreck/sideways_backhalf",
    "shipwreck/rightsideup_full",
    "shipwreck/rightsideup_fronthalf",
    "shipwreck/rightsideup_backhalf",
    "shipwreck/with_mast_degraded",
    "shipwreck/rightsideup_full_degraded",
    "shipwreck/rightsideup_fronthalf_degraded",
    "shipwreck/rightsideup_backhalf_degraded",
];

static OCEAN: &[&str] = &[
    "shipwreck/with_mast",
    "shipwreck/upsidedown_full",
    "shipwreck/upsidedown_fronthalf",
    "shipwreck/upsidedown_backhalf",
    "shipwreck/sideways_full",
    "shipwreck/sideways_fronthalf",
    "shipwreck/sideways_backhalf",
    "shipwreck/rightsideup_full",
    "shipwreck/rightsideup_fronthalf",
    "shipwreck/rightsideup_backhalf",
    "shipwreck/with_mast_degraded",
    "shipwreck/upsidedown_full_degraded",
    "shipwreck/upsidedown_fronthalf_degraded",
    "shipwreck/upsidedown_backhalf_degraded",
    "shipwreck/sideways_full_degraded",
    "shipwreck/sideways_fronthalf_degraded",
    "shipwreck/sideways_backhalf_degraded",
    "shipwreck/rightsideup_full_degraded",
    "shipwreck/rightsideup_fronthalf_degraded",
    "shipwreck/rightsideup_backhalf_degraded",
];

const fn make_shipwreck_piece(
    template_id: Identifier,
    position: (i32, i32, i32),
    rotation: Rotation,
    size: [i32; 3],
    is_beached: bool,
) -> StructurePiece {
    StructurePiece {
        piece_type: Identifier::new_static("minecraft", "shipwreck"),
        bounding_box: rotation.get_bounding_box_with_pivot(
            position,
            (size[0], size[1], size[2]),
            4,
            15,
        ),
        gen_depth: 0,
        orientation: Some(Direction::North),
        payload: StructurePiecePayload::Template(TemplatePieceData {
            template_id,
            template_position: position,
            rotation,
            mirror: StructureMirror::None,
            rotation_pivot: (4, 0, 15),
            block_ignore: StructureBlockIgnore::StructureAndAir,
            late_block_ignore: StructureBlockIgnore::None,
            processors: TemplateProcessorList::Empty,
            liquid_settings: LiquidSettingsData::ApplyWaterlogging,
            marker_handling: TemplateMarkerHandling::Shipwreck,
            placement_adjustment: TemplatePlacementAdjustment::Shipwreck {
                is_beached,
                height_adjusted: false,
            },
            placement_clip: TemplatePlacementClip::CenterChunk,
            post_process: TemplatePostProcess::None,
        }),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    }
}

/// Registered under `"minecraft:shipwreck"`. Beached vs underwater is distinguished by
/// `entry.structure.path`.
pub struct ShipwreckStructure;

impl Structure for ShipwreckStructure {
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

        let StructureConfigData::Shipwreck { is_beached } = &structure.config else {
            return None;
        };
        let templates_arr = if *is_beached { BEACHED } else { OCEAN };

        let rotation = Rotation::get_random(rng);
        let idx = rng.next_i32_bounded(templates_arr.len() as i32) as usize;
        let template_id = Identifier::vanilla_static(templates_arr[idx]);
        let tmpl = ctx.templates().get(&template_id)?;
        let position = (ctx.chunk_min_x(), 90, ctx.chunk_min_z());

        Some(GenerationStub {
            position: (ctx.center_block_x(), surface_y, ctx.center_block_z()),
            pieces: vec![make_shipwreck_piece(
                template_id,
                position,
                rotation,
                tmpl.size,
                *is_beached,
            )],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipwreck_piece_uses_template_payload_with_height_adjustment() {
        let position = (32, 90, -48);
        let size = [15, 12, 28];
        let piece = make_shipwreck_piece(
            Identifier::vanilla_static("shipwreck/with_mast"),
            position,
            Rotation::Clockwise180,
            size,
            true,
        );

        assert_eq!(
            piece.piece_type,
            Identifier::new_static("minecraft", "shipwreck")
        );
        assert_eq!(piece.gen_depth, 0);
        assert_eq!(piece.orientation, Some(Direction::North));
        assert_eq!(
            piece.bounding_box,
            Rotation::Clockwise180.get_bounding_box_with_pivot(
                position,
                (size[0], size[1], size[2]),
                4,
                15,
            ),
        );

        let StructurePiecePayload::Template(data) = piece.payload else {
            panic!("shipwreck piece should be template-backed");
        };
        assert_eq!(
            data.template_id,
            Identifier::vanilla_static("shipwreck/with_mast")
        );
        assert_eq!(data.template_position, position);
        assert_eq!(data.rotation, Rotation::Clockwise180);
        assert_eq!(data.mirror, StructureMirror::None);
        assert_eq!(data.rotation_pivot, (4, 0, 15));
        assert_eq!(data.block_ignore, StructureBlockIgnore::StructureAndAir);
        assert_eq!(data.late_block_ignore, StructureBlockIgnore::None);
        assert_eq!(data.processors, TemplateProcessorList::Empty);
        assert_eq!(data.liquid_settings, LiquidSettingsData::ApplyWaterlogging);
        assert_eq!(data.marker_handling, TemplateMarkerHandling::Shipwreck);
        assert_eq!(
            data.placement_adjustment,
            TemplatePlacementAdjustment::Shipwreck {
                is_beached: true,
                height_adjusted: false,
            }
        );
        assert_eq!(data.placement_clip, TemplatePlacementClip::CenterChunk);
        assert_eq!(data.post_process, TemplatePostProcess::None);
    }
}
