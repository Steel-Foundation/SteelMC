//! Feature-stage structure piece placement boundary.
//!
//! Structure starts are generated before noise, but vanilla emits the piece
//! blocks during biome decoration. This module is the single dispatch point for
//! that pass; individual family placers must fill in exact vanilla behavior
//! before any payload variant starts writing blocks.

mod mineshaft;

use steel_registry::structure::LiquidSettingsData;
use steel_registry::structure_processor::StructureProcessorKind;
use steel_registry::template_pool::{PoolElement, ProcessorList, Projection};
use steel_registry::{Registry, RegistryExt};
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::{BlockPos, BoundingBox, Rotation, types::UpdateFlags};

use crate::world::structure::{ProceduralPieceData, StructurePiece, StructurePiecePayload};
use crate::worldgen::feature::FeatureDecorationRunner;
use crate::worldgen::region::WorldGenRegion;
use crate::worldgen::template::{
    StructureBlockIgnore, StructurePlaceSettings, StructureProcessorRandom, StructureTemplate,
};

pub(crate) struct StructurePiecePlacer;

impl StructurePiecePlacer {
    /// Vanilla jigsaw pool-element placement flags: `UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE`.
    pub(crate) const JIGSAW_UPDATE_FLAGS: UpdateFlags =
        UpdateFlags::UPDATE_CLIENTS.union(UpdateFlags::UPDATE_KNOWN_SHAPE);
    /// Vanilla template-piece placement flags: `UPDATE_CLIENTS`.
    pub(crate) const TEMPLATE_UPDATE_FLAGS: UpdateFlags = UpdateFlags::UPDATE_CLIENTS;

    /// Places one already-clipped structure piece.
    ///
    /// Returns whether the vanilla placement call succeeded. Later milestones
    /// must implement each remaining payload variant completely before it can
    /// return `true`.
    pub(crate) fn place_piece(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        piece: &mut StructurePiece,
        reference_pos: BlockPos,
        clip: BoundingBox,
        random: &mut WorldgenRandom,
        biome_zoom_seed: i64,
    ) -> bool {
        let placed = match &mut piece.payload {
            StructurePiecePayload::Jigsaw(data) => Self::place_pool_element(
                region,
                registry,
                &data.pool_element,
                BlockPos::new(data.position.0, data.position.1, data.position.2),
                reference_pos,
                data.rotation,
                clip,
                random,
                data.liquid_settings,
                biome_zoom_seed,
            ),
            StructurePiecePayload::Template(_data) => {
                let _flags = Self::TEMPLATE_UPDATE_FLAGS;
                // TODO: Implement full vanilla template-backed structure placement.
                false
            }
            StructurePiecePayload::Procedural(ProceduralPieceData::Mineshaft(data)) => {
                Self::place_mineshaft_piece(
                    region,
                    registry,
                    piece.bounding_box,
                    piece.orientation,
                    data,
                    clip,
                    random,
                    biome_zoom_seed,
                )
            }
            StructurePiecePayload::Procedural(ProceduralPieceData::Unimplemented) => false,
        };
        placed
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors StructurePoolElement.place inputs"
    )]
    fn place_pool_element(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        element: &PoolElement,
        position: BlockPos,
        reference_pos: BlockPos,
        rotation: Rotation,
        clip: BoundingBox,
        random: &mut WorldgenRandom,
        liquid_settings: LiquidSettingsData,
        biome_zoom_seed: i64,
    ) -> bool {
        match element {
            PoolElement::Single {
                location,
                processors,
                projection,
            } => Self::place_single_pool_element(
                region,
                registry,
                location,
                processors,
                *projection,
                StructureBlockIgnore::StructureBlock,
                StructureBlockIgnore::None,
                position,
                reference_pos,
                rotation,
                clip,
                random,
                liquid_settings,
            ),
            PoolElement::LegacySingle {
                location,
                processors,
                projection,
            } => Self::place_single_pool_element(
                region,
                registry,
                location,
                processors,
                *projection,
                StructureBlockIgnore::None,
                StructureBlockIgnore::StructureAndAir,
                position,
                reference_pos,
                rotation,
                clip,
                random,
                liquid_settings,
            ),
            PoolElement::Empty => true,
            PoolElement::Feature { feature, .. } => {
                FeatureDecorationRunner::place_structure_pool_feature(
                    region,
                    registry,
                    random,
                    position,
                    feature,
                    biome_zoom_seed,
                )
            }
            PoolElement::List { elements, .. } => {
                for element in elements {
                    if !Self::place_pool_element(
                        region,
                        registry,
                        element,
                        position,
                        reference_pos,
                        rotation,
                        clip,
                        random,
                        liquid_settings,
                        biome_zoom_seed,
                    ) {
                        return false;
                    }
                }
                true
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors SinglePoolElement.place and StructureTemplate.placeInWorld"
    )]
    fn place_single_pool_element(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        location: &steel_utils::Identifier,
        processors: &ProcessorList,
        projection: Projection,
        block_ignore: StructureBlockIgnore,
        late_block_ignore: StructureBlockIgnore,
        position: BlockPos,
        reference_pos: BlockPos,
        rotation: Rotation,
        clip: BoundingBox,
        random: &mut WorldgenRandom,
        liquid_settings: LiquidSettingsData,
    ) -> bool {
        let template = match StructureTemplate::load_vanilla(registry, location) {
            Ok(template) => template,
            Err(err) => panic!("{err}"),
        };
        let processor_list = Self::processors(registry, processors);
        let settings = StructurePlaceSettings {
            rotation,
            bounding_box: clip,
            processors: processor_list,
            block_ignore,
            late_block_ignore,
            replace_jigsaws: true,
            projection: Some(projection),
            processor_random: StructureProcessorRandom::Positional,
            liquid_settings,
        };

        template.place_in_world(
            region,
            registry,
            position,
            reference_pos,
            &settings,
            random,
            Self::JIGSAW_UPDATE_FLAGS,
        )
    }

    fn processors<'a>(
        registry: &'a Registry,
        processors: &'a ProcessorList,
    ) -> &'a [StructureProcessorKind] {
        match processors {
            ProcessorList::Empty => &[],
            ProcessorList::Registry(key) => {
                let Some(processor_list) = registry.structure_processors.by_key(key) else {
                    panic!("template pool references unknown processor list {key}");
                };
                &processor_list.data.processors
            }
        }
    }
}
