//! Woodland mansion. Vanilla's `WoodlandMansionPieces`: grid-based layout with
//! template pieces for walls, corridors, rooms, and roofs.

use glam::IVec3;
use steel_registry::structure::{LiquidSettingsData, StructureData};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Direction, Identifier, Rotation};

use crate::structure::{
    GenerationStub, Structure, StructureBlockIgnore, StructureGenerationContext, StructureMirror,
    StructurePiece, StructurePiecePayload, TemplateMarkerHandling, TemplatePieceData,
    TemplatePlacementAdjustment, TemplatePlacementClip, TemplatePostProcess, TemplateProcessorList,
};

mod grid;
mod placement;
mod roof;
mod rooms;
mod template;
mod walls;

#[cfg(test)]
mod tests;

use placement::generate_mansion_pieces;
use template::MansionTemplatePiece;

/// `Structure` impl — registered under `"minecraft:woodland_mansion"`.
///
/// Vanilla's `WoodlandMansionStructure.findGenerationPoint`: consumes a
/// rotation, probes a rotation-dependent 5×5 box for the lowest Y, rejects
/// if `< 60`, then runs `generate_mansion_pieces`.
pub struct WoodlandMansionStructure;

impl Structure for WoodlandMansionStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let (bx, bz) = (ctx.chunk_min_x() + 7, ctx.chunk_min_z() + 7);
        if !ctx.could_valid_biome_exist_in_terrain_column(bx, bz, &structure.allowed_biomes) {
            return None;
        }
        let rotation = Rotation::get_random(rng);

        let (off_x, off_z) = match rotation {
            Rotation::None => (5, 5),
            Rotation::Clockwise90 => (-5, 5),
            Rotation::Clockwise180 => (-5, -5),
            Rotation::CounterClockwise90 => (5, -5),
        };
        // `getLowestY` samples the first occupied height at the four corners.
        let h0 = ctx.base_height(bx, bz, false) - 1;
        let h1 = ctx.base_height(bx, bz + off_z, false) - 1;
        let h2 = ctx.base_height(bx + off_x, bz, false) - 1;
        let h3 = ctx.base_height(bx + off_x, bz + off_z, false) - 1;
        let lowest = h0.min(h1).min(h2).min(h3);
        if lowest < 60 {
            return None;
        }

        let biome = ctx.biome_at(bx, lowest, bz);
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let origin = IVec3::new(bx, lowest, bz);
        let pieces = generate_mansion_pieces(origin, rotation, rng)
            .into_iter()
            .map(MansionTemplatePiece::into_structure_piece)
            .collect();

        Some(GenerationStub {
            position: (origin.x, origin.y, origin.z),
            pieces,
        })
    }
}
