//! Nether fossil: sample a random (x, z) in the chunk and a uniform Y, then walk
//! the base-noise column down until we find air over solid. Fails if the walk
//! reaches sea level.

use steel_registry::structure::{
    HeightProviderData, LiquidSettingsData, StructureConfigData, StructureData, VerticalAnchorData,
};
use steel_utils::Direction;
use steel_utils::Identifier;
use steel_utils::Rotation;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;

use crate::structure::{
    GenerationStub, Structure, StructureBlockIgnore, StructureGenerationContext, StructureMirror,
    StructurePiece, StructurePiecePayload, TemplateMarkerHandling, TemplatePieceData,
    TemplatePlacementAdjustment, TemplatePlacementClip, TemplatePostProcess, TemplateProcessorList,
};

/// Fossil templates count (`minecraft:nether_fossils/fossil_N`).
pub const FOSSIL_COUNT: i32 = 14;
const SEA_LEVEL: i32 = 32;

/// Result of [`find_generation_point`].
pub struct FossilResult {
    /// Template path relative to `minecraft:` (e.g. `"nether_fossils/fossil_3"`).
    pub template_name: String,
    /// World-space solid-block position.
    pub position: (i32, i32, i32),
    /// Piece rotation.
    pub rotation: Rotation,
    /// Position used for the biome check.
    pub biome_check_pos: (i32, i32, i32),
}

const fn resolve_vertical_anchor(
    anchor: &VerticalAnchorData,
    min_gen_y: i32,
    gen_depth: i32,
) -> i32 {
    match anchor {
        VerticalAnchorData::Absolute(y) => *y,
        VerticalAnchorData::AboveBottom(offset) => min_gen_y + *offset,
        VerticalAnchorData::BelowTop(offset) => min_gen_y + gen_depth - 1 - *offset,
    }
}

fn sample_height(
    height: &HeightProviderData,
    rng: &mut LegacyRandom,
    min_gen_y: i32,
    gen_depth: i32,
) -> i32 {
    match height {
        HeightProviderData::Constant(anchor) => {
            resolve_vertical_anchor(anchor, min_gen_y, gen_depth)
        }
        HeightProviderData::Uniform {
            min_inclusive,
            max_inclusive,
        } => {
            let min = resolve_vertical_anchor(min_inclusive, min_gen_y, gen_depth);
            let max = resolve_vertical_anchor(max_inclusive, min_gen_y, gen_depth);
            if min > max {
                min
            } else {
                min + rng.next_i32_bounded(max - min + 1)
            }
        }
    }
}

/// Vanilla's RNG sequence. Returns `None` if no air-over-solid transition above sea level.
pub fn find_generation_point<F>(
    rng: &mut LegacyRandom,
    chunk_x: i32,
    chunk_z: i32,
    height: &HeightProviderData,
    min_gen_y: i32,
    gen_depth: i32,
    mut solid_block_below_air: F,
) -> Option<FossilResult>
where
    F: FnMut(i32, i32, i32, i32) -> Option<i32>,
{
    let block_x = (chunk_x << 4) + rng.next_i32_bounded(16);
    let block_z = (chunk_z << 4) + rng.next_i32_bounded(16);

    let start_y = sample_height(height, rng, min_gen_y, gen_depth);
    let y = solid_block_below_air(block_x, block_z, start_y, SEA_LEVEL + 1)?;

    let rotation = Rotation::get_random(rng);
    let fossil_idx = rng.next_i32_bounded(FOSSIL_COUNT) + 1;
    Some(FossilResult {
        template_name: format!("nether_fossils/fossil_{fossil_idx}"),
        position: (block_x, y, block_z),
        rotation,
        biome_check_pos: (block_x, y, block_z),
    })
}

const fn make_nether_fossil_piece(
    template_id: Identifier,
    position: (i32, i32, i32),
    rotation: Rotation,
    size: [i32; 3],
) -> StructurePiece {
    StructurePiece {
        piece_type: Identifier::new_static("minecraft", "nefos"),
        bounding_box: rotation.get_bounding_box(
            position.0, position.1, position.2, size[0], size[1], size[2],
        ),
        gen_depth: 0,
        orientation: Some(Direction::North),
        payload: StructurePiecePayload::Template(TemplatePieceData {
            template_id,
            template_position: position,
            rotation,
            mirror: StructureMirror::None,
            rotation_pivot: (0, 0, 0),
            block_ignore: StructureBlockIgnore::StructureAndAir,
            late_block_ignore: StructureBlockIgnore::None,
            processors: TemplateProcessorList::Empty,
            liquid_settings: LiquidSettingsData::ApplyWaterlogging,
            marker_handling: TemplateMarkerHandling::Ignore,
            placement_adjustment: TemplatePlacementAdjustment::None,
            placement_clip: TemplatePlacementClip::CenterChunkExpandedToTemplate,
            post_process: TemplatePostProcess::NetherFossil,
        }),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    }
}

/// Entry point used by `VanillaGenerator`.
pub struct NetherFossilStructure;

impl Structure for NetherFossilStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let min_gen_y = ctx.min_y();
        let gen_depth = ctx.height();
        let (chunk_x, chunk_z) = (ctx.chunk_x(), ctx.chunk_z());
        let StructureConfigData::NetherFossil { height } = &structure.config else {
            return None;
        };

        let result = find_generation_point(
            rng,
            chunk_x,
            chunk_z,
            height,
            min_gen_y,
            gen_depth,
            |x, z, start_y, min_solid_y| ctx.solid_block_below_air(x, z, start_y, min_solid_y),
        )?;

        let (bx, by, bz) = result.biome_check_pos;
        let biome = ctx.biome_at(bx, by, bz);
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let template_id = Identifier::vanilla(result.template_name);
        let tmpl_size = ctx.templates().get(&template_id)?.size;
        Some(GenerationStub {
            position: result.position,
            pieces: vec![make_nether_fossil_piece(
                template_id,
                result.position,
                result.rotation,
                tmpl_size,
            )],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nether_fossil_piece_uses_full_template_payload() {
        let position = (10, 45, -20);
        let size = [7, 5, 9];
        let piece = make_nether_fossil_piece(
            Identifier::vanilla_static("nether_fossils/fossil_1"),
            position,
            Rotation::Clockwise90,
            size,
        );

        assert_eq!(
            piece.piece_type,
            Identifier::new_static("minecraft", "nefos")
        );
        assert_eq!(piece.gen_depth, 0);
        assert_eq!(piece.orientation, Some(Direction::North));
        assert_eq!(
            piece.bounding_box,
            Rotation::Clockwise90.get_bounding_box(
                position.0, position.1, position.2, size[0], size[1], size[2],
            ),
        );

        let StructurePiecePayload::Template(data) = piece.payload else {
            panic!("nether fossil piece should be template-backed");
        };
        assert_eq!(
            data.template_id,
            Identifier::vanilla_static("nether_fossils/fossil_1")
        );
        assert_eq!(data.template_position, position);
        assert_eq!(data.rotation, Rotation::Clockwise90);
        assert_eq!(data.mirror, StructureMirror::None);
        assert_eq!(data.rotation_pivot, (0, 0, 0));
        assert_eq!(data.block_ignore, StructureBlockIgnore::StructureAndAir);
        assert_eq!(data.late_block_ignore, StructureBlockIgnore::None);
        assert_eq!(data.processors, TemplateProcessorList::Empty);
        assert_eq!(data.liquid_settings, LiquidSettingsData::ApplyWaterlogging);
        assert_eq!(data.marker_handling, TemplateMarkerHandling::Ignore);
        assert_eq!(data.placement_adjustment, TemplatePlacementAdjustment::None);
        assert_eq!(
            data.placement_clip,
            TemplatePlacementClip::CenterChunkExpandedToTemplate
        );
        assert_eq!(data.post_process, TemplatePostProcess::NetherFossil);
    }
}
