//! Structure start/reference tracking.
//!
//! Vanilla keeps two per-chunk maps: `structureStarts` (originating here) and
//! `structuresReferences` (pointing at nearby origin chunks). The structure key
//! is `Identifier` until a structure registry is added.

use steel_registry::structure::StructureData;
use steel_utils::random::legacy_random::LegacyRandom;

mod box_octree;
pub mod desert_pyramid;
pub mod end_city;
pub mod fortress;
/// Structure piece generation context.
pub mod generation;
/// Structure placement/selection engine.
pub mod generator;
pub mod igloo;
pub mod jigsaw;
pub mod jungle_temple;
pub mod mansion;
pub mod mineshaft;
pub mod nether_fossil;
pub mod ocean_monument;
pub mod ocean_ruin;
mod piece;
pub mod placement;
pub mod ruined_portal;
pub mod shipwreck;
pub mod single_piece;
/// Structure starts and references.
pub mod start;
pub mod stronghold;
pub mod swamp_hut;
/// Miscellaneous structure utility functions.
pub mod utils;

pub use piece::{
    ProceduralPieceData, RuinedPortalProperties, StructureBlockIgnore, StructureMirror,
    StructurePiece, StructurePiecePayload, TemplateMarkerHandling, TemplatePieceData,
    TemplatePlacementAdjustment, TemplatePlacementClip, TemplatePostProcess, TemplateProcessorList,
};

pub use generation::{ColumnBlock, GenerationContext, GenerationStub, StructureGenerationContext};
pub use generator::{
    FixedStructureBiomeProvider, StructureBiomeProvider, StructureGenerator,
    StructureGeneratorAssets, StructureLocateCandidate, StructureLocatePlacement,
    StructureLocatePlan, squared_distance,
};
pub use start::{StructureReferenceMap, StructureReferenceSet, StructureStart, StructureStartMap};
pub(crate) use utils::{make_oriented_piece_bounding_box, random_horizontal_direction};

/// Vanilla's `Structure::findValidGenerationPoint`. Impls own their RNG order,
/// collision checks, and biome check.
pub trait Structure: Send + Sync {
    /// `structure` carries registry data; per-set metadata stays in placement.
    /// `rng` is a fresh `LegacyRandom` seeded with `setLargeFeatureSeed`.
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub>;
}
