//! Runtime structure-piece payload model used by feature-stage placement.

use steel_registry::structure::{
    LiquidSettingsData, OceanRuinBiomeTempData, RuinedPortalPlacementData,
};
use steel_registry::template_pool::Projection;
use steel_utils::{BoundingBox, Direction, Identifier, Rotation};

use super::{
    desert_pyramid, fortress, jigsaw, jungle_temple, mineshaft, ocean_monument, stronghold,
    swamp_hut,
};

use steel_registry::Registry;
use steel_registry::vanilla_blocks;
use steel_utils::BlockStateId;

/// Vanilla's `StructurePiece` runtime state.
#[derive(Debug, Clone)]
pub struct StructurePiece {
    /// Piece type id (e.g., `minecraft:jigsaw`).
    pub piece_type: Identifier,
    /// World-space bounding box.
    pub bounding_box: BoundingBox,
    /// Distance from the start piece in the piece tree.
    pub gen_depth: i32,
    /// Horizontal orientation; `None` for unoriented pieces.
    pub orientation: Option<Direction>,
    /// Type-specific data used by the structure-piece placement stage.
    pub payload: StructurePiecePayload,
    /// Offset from piece minY to ground level. Used by Beardifier. Default 0 for non-jigsaw.
    pub ground_level_delta: i32,
    /// Junctions for Beardifier terrain adaptation.
    pub junctions: Vec<jigsaw::JigsawJunction>,
    /// Jigsaw projection. `None` for non-jigsaw pieces.
    ///
    /// Beardifier treats `Some(Rigid)` and `None` as terrain-adapting, but skips
    /// `Some(TerrainMatching)` from the rigid set (still collecting junctions).
    /// Mirrors vanilla's `piece instanceof PoolElementStructurePiece` + `Projection.RIGID` check.
    pub projection: Option<Projection>,
}

impl StructurePiece {
    /// Creates a non-jigsaw piece with vanilla's default non-pool metadata.
    #[must_use]
    pub const fn non_jigsaw(
        piece_type: Identifier,
        bounding_box: BoundingBox,
        gen_depth: i32,
        orientation: Option<Direction>,
    ) -> Self {
        Self {
            piece_type,
            bounding_box,
            gen_depth,
            orientation,
            payload: StructurePiecePayload::Procedural(ProceduralPieceData::Unimplemented),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        }
    }
}

/// Type-specific structure-piece placement payload.
///
/// This is Steel's boundary between structure-start generation and feature-stage
/// block placement. Common vanilla fields stay on [`StructurePiece`]; placement
/// implementations dispatch on this enum instead of inferring behavior from a
/// bounding box or legacy NBT blob.
#[derive(Debug, Clone)]
pub enum StructurePiecePayload {
    /// Pool piece produced by jigsaw assembly.
    Jigsaw(jigsaw::JigsawPieceData),
    /// Template-backed vanilla piece outside the jigsaw system.
    Template(TemplatePieceData),
    /// Code-generated piece family whose blocks are emitted procedurally.
    Procedural(ProceduralPieceData),
}

/// Template-backed non-jigsaw placement data.
#[derive(Debug, Clone)]
pub struct TemplatePieceData {
    /// Structure template identifier.
    pub template_id: Identifier,
    /// World-space template origin before rotation/mirror transforms.
    pub template_position: (i32, i32, i32),
    /// Template rotation.
    pub rotation: Rotation,
    /// Template mirror mode.
    pub mirror: StructureMirror,
    /// Rotation pivot in template-local block coordinates.
    pub rotation_pivot: (i32, i32, i32),
    /// Block-ignore processor applied before the registry processor list.
    pub block_ignore: StructureBlockIgnore,
    /// Block-ignore processor applied after the registry processor list.
    pub late_block_ignore: StructureBlockIgnore,
    /// Processor list applied during placement.
    pub processors: TemplateProcessorList,
    /// Liquid handling mode used by vanilla template placement.
    pub liquid_settings: LiquidSettingsData,
    /// How structure-template data markers are handled for this family.
    pub marker_handling: TemplateMarkerHandling,
    /// Family-specific position adjustment before template block placement.
    pub placement_adjustment: TemplatePlacementAdjustment,
    /// Bounding box passed to vanilla template placement.
    pub placement_clip: TemplatePlacementClip,
    /// Family-specific work done after the template blocks are placed.
    pub post_process: TemplatePostProcess,
}

/// Processors for template-backed non-jigsaw pieces.
#[derive(Debug, Clone, PartialEq)]
pub enum TemplateProcessorList {
    /// No processors.
    Empty,
    /// Registry-backed vanilla processor list.
    Registry(Identifier),
    /// Vanilla's hardcoded ocean-ruin block-rot and archaeology processors.
    OceanRuin {
        /// Warm/cold ruin variant controls suspicious sand/gravel and archaeology loot.
        biome_temp: OceanRuinBiomeTempData,
        /// `BlockRotProcessor` keep probability.
        integrity: f32,
    },
    /// Vanilla's hardcoded ruined-portal processor sequence.
    RuinedPortal {
        /// Vertical placement controls lava replacement.
        vertical_placement: RuinedPortalPlacementData,
        /// Ruined portal setup properties.
        properties: RuinedPortalProperties,
    },
}

/// Vanilla ruined-portal piece properties used by processors and postprocess.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuinedPortalProperties {
    /// Whether cold lava/netherrack behavior is active.
    pub cold: bool,
    /// Vanilla block-age processor mossiness.
    pub mossiness: f32,
    /// Whether structure air is preserved.
    pub air_pocket: bool,
    /// Whether netherrack can grow jungle leaves.
    pub overgrown: bool,
    /// Whether vines can be added to sturdy sides.
    pub vines: bool,
    /// Whether stone ruin blocks are replaced with blackstone variants.
    pub replace_with_blackstone: bool,
}

/// Vanilla `Mirror` modes used by template placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureMirror {
    /// No mirror transform.
    None,
    /// Mirror across the template front/back axis.
    FrontBack,
    /// Mirror across the template left/right axis.
    LeftRight,
}

/// Hardcoded vanilla block-ignore processors used by template placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureBlockIgnore {
    /// Do not ignore any block states.
    None,
    /// Ignore structure blocks.
    StructureBlock,
    /// Ignore structure blocks and air.
    StructureAndAir,
}

impl StructureBlockIgnore {
    /// Checks whether the block state should be ignored during placement.
    ///
    /// # Panics
    ///
    /// Panics if the given block state ID is not registered in the blocks registry.
    #[must_use]
    pub fn ignores(self, registry: &Registry, state: BlockStateId) -> bool {
        match self {
            Self::None => false,
            Self::StructureBlock => {
                registry.blocks.by_state_id(state).expect("invalid state")
                    == &vanilla_blocks::STRUCTURE_BLOCK
            }
            Self::StructureAndAir => {
                let block = registry.blocks.by_state_id(state).expect("invalid state");
                block == &vanilla_blocks::STRUCTURE_BLOCK || block == &vanilla_blocks::AIR
            }
        }
    }
}

/// Marker handling requested by a template-backed structure piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateMarkerHandling {
    /// Ignore data markers.
    Ignore,
    /// Dispatch data markers to the structure-family placement code.
    DataMarkers,
    /// Ocean ruin chest and drowned markers.
    OceanRuin {
        /// Whether the ruin uses the large chest loot table.
        is_large: bool,
    },
    /// Shipwreck map, supply, and treasure chest markers.
    Shipwreck,
    /// Igloo basement chest marker.
    Igloo,
    /// End-city chest, shulker, and Elytra frame markers.
    EndCity,
    /// Woodland mansion chest, illager, and allay markers.
    WoodlandMansion,
}

/// Family-specific template position adjustment before block placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplatePlacementAdjustment {
    /// Place at the persisted template position.
    None,
    /// Shipwreck height adjustment, persisted after the first placement call.
    Shipwreck {
        /// Whether this is the beached shipwreck variant.
        is_beached: bool,
        /// Vanilla `height_adjusted` flag.
        height_adjusted: bool,
    },
    /// Igloo per-placement height adjustment.
    Igloo {
        /// Vanilla template offset for this igloo piece.
        template_offset: (i32, i32, i32),
    },
    /// Ocean ruin terrain height adjustment.
    OceanRuin,
}

/// Vanilla bounding box adjustment before calling `StructureTemplate.placeInWorld`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::enum_variant_names,
    reason = "variants intentionally name the center-chunk clipping basis used by vanilla structure placement"
)]
pub enum TemplatePlacementClip {
    /// Use the center chunk's writable box unchanged.
    CenterChunk,
    /// Expand the center chunk writable box to include this piece's transformed template box.
    CenterChunkExpandedToTemplate,
    /// Expand to the transformed template box only when its center is in the center chunk.
    CenterChunkContainsTemplateCenterExpandedToTemplate,
}

/// Family-specific post-template processing for template-backed pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplatePostProcess {
    /// No family-specific postprocess.
    None,
    /// Nether fossil dried-ghast placement.
    NetherFossil,
    /// Igloo top-piece trapdoor snow-block fixup.
    IglooTop,
    /// Ruined portal netherrack spread, drip columns, vines, and overgrowth.
    RuinedPortal,
}

/// Family-specific state for code-generated structure pieces.
#[derive(Debug, Clone)]
pub enum ProceduralPieceData {
    /// Procedural family whose placement implementation has not been enabled yet.
    Unimplemented,
    /// Buried treasure chest placement.
    BuriedTreasure,
    /// Desert pyramid piece payload.
    DesertPyramid(desert_pyramid::DesertPyramidPieceData),
    /// Jungle temple piece payload.
    JungleTemple(jungle_temple::JungleTemplePieceData),
    /// Mineshaft room/corridor/crossing/stairs payload.
    Mineshaft(mineshaft::MineshaftPiecePayload),
    /// Nether fortress bridge/castle piece payload.
    NetherFortress(fortress::FortressPieceData),
    /// Ocean monument building payload.
    OceanMonument(ocean_monument::OceanMonumentPieceData),
    /// Stronghold recursive piece payload.
    Stronghold(stronghold::StrongholdPieceData),
    /// Swamp hut piece payload.
    SwampHut(swamp_hut::SwampHutPieceData),
}
