//! Structure start/reference tracking.
//!
//! Vanilla keeps two per-chunk maps: `structureStarts` (originating here) and
//! `structuresReferences` (pointing at nearby origin chunks). The structure key
//! is `Identifier` until a structure registry is added.

pub mod desert_pyramid;
pub mod end_city;
pub mod fortress;
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
pub mod stronghold;
pub mod swamp_hut;
/// Structure piece types and generation stubs.
pub mod types;
/// Miscellaneous structure utility functions.
pub mod utils;

pub use piece::{
    ProceduralPieceData, RuinedPortalProperties, StructureBlockIgnore, StructureMirror,
    StructurePiece, StructurePiecePayload, TemplateMarkerHandling, TemplatePieceData,
    TemplatePlacementAdjustment, TemplatePlacementClip, TemplatePostProcess, TemplateProcessorList,
};

pub use generator::{
    FixedStructureBiomeProvider, StructureBiomeProvider, StructureGenerator,
    StructureGeneratorAssets, StructureLocateCandidate, StructureLocatePlacement,
    StructureLocatePlan, squared_distance,
};
pub use types::ColumnBlock;
pub use types::generation::{GenerationContext, GenerationStub, StructureGenerationContext};
pub use types::structure::Structure;
pub use types::structure_ref::{
    StructureReferenceMap, StructureReferenceSet, StructureStart, StructureStartMap,
};
pub(crate) use utils::{make_oriented_piece_bounding_box, random_horizontal_direction};
