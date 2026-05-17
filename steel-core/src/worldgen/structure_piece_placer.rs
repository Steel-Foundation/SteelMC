//! Feature-stage structure piece placement boundary.
//!
//! Structure starts are generated before noise, but vanilla emits the piece
//! blocks during biome decoration. This module is the single dispatch point for
//! that pass; individual family placers must fill in exact vanilla behavior
//! before any payload variant starts writing blocks.

mod mineshaft;

use simdnbt::owned::NbtCompound;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::structure::LiquidSettingsData;
use steel_registry::structure_processor::StructureProcessorKind;
use steel_registry::template_pool::{PoolElement, ProcessorList, Projection};
use steel_registry::{Registry, RegistryExt, vanilla_block_entity_types, vanilla_blocks};
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::random::{PositionalRandom, Random};
use steel_utils::{BlockPos, BoundingBox, Rotation, types::UpdateFlags};

use crate::chunk::heightmap::HeightmapType;
use crate::world::structure::{
    ProceduralPieceData, StructureBlockIgnore, StructureMirror, StructurePiece,
    StructurePiecePayload, TemplateMarkerHandling, TemplatePieceData, TemplatePlacementAdjustment,
    TemplatePlacementClip, TemplatePostProcess,
};
use crate::worldgen::feature::FeatureDecorationRunner;
use crate::worldgen::region::WorldGenRegion;
use crate::worldgen::template::{
    StructureDataMarker, StructurePlaceSettings, StructureProcessorRandom, StructureTemplate,
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
        let mut piece_bounding_box = piece.bounding_box;
        let piece_orientation = piece.orientation;
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
            StructurePiecePayload::Template(data) => Self::place_template_piece(
                region,
                registry,
                data,
                &mut piece_bounding_box,
                reference_pos,
                clip,
                random,
            ),
            StructurePiecePayload::Procedural(ProceduralPieceData::Mineshaft(data)) => {
                Self::place_mineshaft_piece(
                    region,
                    registry,
                    piece_bounding_box,
                    piece_orientation,
                    data,
                    clip,
                    random,
                    biome_zoom_seed,
                )
            }
            StructurePiecePayload::Procedural(ProceduralPieceData::Unimplemented) => false,
        };
        piece.bounding_box = piece_bounding_box;
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
            mirror: StructureMirror::None,
            rotation,
            rotation_pivot: BlockPos::ZERO,
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

    fn place_template_piece(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        data: &mut TemplatePieceData,
        piece_bounding_box: &mut BoundingBox,
        reference_pos: BlockPos,
        clip: BoundingBox,
        random: &mut WorldgenRandom,
    ) -> bool {
        if data.marker_handling == TemplateMarkerHandling::DataMarkers {
            // TODO: Add family-specific data marker dispatch before enabling these pieces.
            return false;
        }

        let template = match StructureTemplate::load_vanilla(registry, &data.template_id) {
            Ok(template) => template,
            Err(err) => panic!("{err}"),
        };
        Self::adjust_template_position(region, &template, data, random);
        let processor_list = Self::processors(registry, &data.processors);
        let position = BlockPos::new(
            data.template_position.0,
            data.template_position.1,
            data.template_position.2,
        );
        let settings = StructurePlaceSettings {
            mirror: data.mirror,
            rotation: data.rotation,
            rotation_pivot: BlockPos::new(
                data.rotation_pivot.0,
                data.rotation_pivot.1,
                data.rotation_pivot.2,
            ),
            bounding_box: clip,
            processors: processor_list,
            block_ignore: data.block_ignore,
            late_block_ignore: data.late_block_ignore,
            replace_jigsaws: false,
            projection: None,
            processor_random: StructureProcessorRandom::Positional,
            liquid_settings: data.liquid_settings,
        };
        let template_box = template.bounding_box_with_transform(
            position,
            data.rotation,
            data.mirror,
            settings.rotation_pivot,
        );
        *piece_bounding_box = template_box;
        let placement_clip = Self::template_placement_clip(data.placement_clip, clip, template_box);
        let settings = StructurePlaceSettings {
            bounding_box: placement_clip,
            ..settings
        };
        if !template_box.intersects(&placement_clip) {
            return false;
        }

        let placed = template.place_in_world(
            region,
            registry,
            position,
            reference_pos,
            &settings,
            random,
            Self::TEMPLATE_UPDATE_FLAGS,
        );
        if placed {
            if !Self::handle_template_data_markers(
                region,
                registry,
                &template,
                data.marker_handling,
                position,
                &settings,
                random,
            ) {
                return false;
            }
            template.replace_jigsaw_final_states(region, registry, position, &settings, random);
            Self::post_process_template_piece(
                region,
                registry,
                data.post_process,
                template_box,
                placement_clip,
            );
        }
        placed
    }

    fn adjust_template_position(
        region: &WorldGenRegion<'_>,
        template: &StructureTemplate,
        data: &mut TemplatePieceData,
        random: &mut WorldgenRandom,
    ) {
        match &mut data.placement_adjustment {
            TemplatePlacementAdjustment::None => {}
            TemplatePlacementAdjustment::Shipwreck {
                is_beached,
                height_adjusted,
            } => {
                if *height_adjusted || Self::shipwreck_is_too_big_to_fit(template) {
                    return;
                }
                let new_y = Self::adjusted_shipwreck_y(
                    region,
                    template,
                    data.template_position,
                    *is_beached,
                    random,
                );
                data.template_position.1 = new_y;
                *height_adjusted = true;
            }
        }
    }

    fn shipwreck_is_too_big_to_fit(template: &StructureTemplate) -> bool {
        let size = template.size(Rotation::None);
        size[0] > 32 || size[1] > 32
    }

    fn adjusted_shipwreck_y(
        region: &WorldGenRegion<'_>,
        template: &StructureTemplate,
        position: (i32, i32, i32),
        is_beached: bool,
        random: &mut WorldgenRandom,
    ) -> i32 {
        let size = template.size(Rotation::None);
        let heightmap_type = if is_beached {
            HeightmapType::WorldSurfaceWg
        } else {
            HeightmapType::OceanFloorWg
        };
        let base_area = size[0] * size[2];
        if base_area == 0 {
            return region.height_at(heightmap_type, position.0, position.2);
        }

        let mut min_y = region.max_y_exclusive();
        let mut mean = 0;
        for z in position.2..position.2 + size[2] {
            for x in position.0..position.0 + size[0] {
                let height = region.height_at(heightmap_type, x, z);
                mean += height;
                min_y = min_y.min(height);
            }
        }
        mean /= base_area;

        if is_beached {
            min_y - size[1] / 2 - random.next_i32_bounded(3)
        } else {
            mean
        }
    }

    fn handle_template_data_markers(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        template: &StructureTemplate,
        marker_handling: TemplateMarkerHandling,
        position: BlockPos,
        settings: &StructurePlaceSettings<'_>,
        random: &mut WorldgenRandom,
    ) -> bool {
        match marker_handling {
            TemplateMarkerHandling::Ignore => true,
            TemplateMarkerHandling::DataMarkers => {
                // TODO: Add family-specific data marker dispatch before enabling these pieces.
                false
            }
            TemplateMarkerHandling::Shipwreck => {
                for marker in template.data_markers(registry, position, settings, random) {
                    Self::handle_shipwreck_marker(region, &marker, random);
                }
                true
            }
        }
    }

    fn handle_shipwreck_marker(
        region: &mut WorldGenRegion<'_>,
        marker: &StructureDataMarker,
        random: &mut WorldgenRandom,
    ) {
        let loot_table = match marker.metadata.as_str() {
            "map_chest" => "minecraft:chests/shipwreck_map",
            "treasure_chest" => "minecraft:chests/shipwreck_treasure",
            "supply_chest" => "minecraft:chests/shipwreck_supply",
            _ => return,
        };
        let chest_pos = marker.pos.below();
        let state = region.block_state(chest_pos);
        if state.get_block() != &vanilla_blocks::CHEST {
            return;
        }

        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", loot_table);
        nbt.insert("LootTableSeed", random.next_i64());
        let _ =
            region.set_block_entity_data(chest_pos, &vanilla_block_entity_types::CHEST, state, nbt);
    }

    const fn template_placement_clip(
        placement_clip: TemplatePlacementClip,
        center_clip: BoundingBox,
        template_box: BoundingBox,
    ) -> BoundingBox {
        match placement_clip {
            TemplatePlacementClip::CenterChunk => center_clip,
            TemplatePlacementClip::CenterChunkExpandedToTemplate => {
                BoundingBox::encapsulating(&center_clip, &template_box)
            }
        }
    }

    fn post_process_template_piece(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        post_process: TemplatePostProcess,
        template_box: BoundingBox,
        placement_clip: BoundingBox,
    ) {
        match post_process {
            TemplatePostProcess::None => {}
            TemplatePostProcess::NetherFossil => {
                Self::place_nether_fossil_dried_ghast(
                    region,
                    registry,
                    template_box,
                    placement_clip,
                );
            }
        }
    }

    fn place_nether_fossil_dried_ghast(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        fossil_box: BoundingBox,
        placement_clip: BoundingBox,
    ) {
        let center = fossil_box.get_center();
        let mut seed_random = LegacyRandom::from_seed(region.seed() as u64);
        let splitter = seed_random.next_positional();
        let mut positional_random = splitter.at(center.x(), center.y(), center.z());
        if positional_random.next_f32() >= 0.5 {
            return;
        }

        let pos = BlockPos::new(
            fossil_box.min_x + positional_random.next_i32_bounded(fossil_box.get_x_span()),
            fossil_box.min_y,
            fossil_box.min_z + positional_random.next_i32_bounded(fossil_box.get_z_span()),
        );
        if !placement_clip.is_inside(pos) {
            return;
        }
        if !region.block_state(pos).is_air() {
            return;
        }

        let rotation = Rotation::get_random(&mut positional_random);
        let state = Self::dried_ghast_state(registry, rotation);
        let _ = region.set_block_state(pos, state, Self::TEMPLATE_UPDATE_FLAGS);
    }

    fn dried_ghast_state(registry: &Registry, rotation: Rotation) -> steel_utils::BlockStateId {
        let facing = rotation.rotate(steel_utils::Direction::North);
        let Some(state) = registry.blocks.state_id_from_block_defaulted_properties(
            &vanilla_blocks::DRIED_GHAST,
            [("facing", facing.as_str())],
        ) else {
            panic!("dried_ghast missing vanilla facing property");
        };
        state
    }
}
