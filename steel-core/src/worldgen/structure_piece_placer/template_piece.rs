use simdnbt::owned::NbtCompound;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::{Registry, vanilla_block_entity_types, vanilla_blocks};
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::random::{PositionalRandom, Random};
use steel_utils::{BlockPos, BlockStateId, BoundingBox, Direction, Rotation, types::UpdateFlags};

use crate::chunk::heightmap::HeightmapType;
use crate::world::structure::{
    StructureMirror, TemplateMarkerHandling, TemplatePieceData, TemplatePlacementAdjustment,
    TemplatePlacementClip, TemplatePostProcess,
};
use crate::worldgen::region::WorldGenRegion;
use crate::worldgen::template::{
    StructureDataMarker, StructurePlaceSettings, StructureProcessorRandom, StructureTemplate,
};

use super::StructurePiecePlacer;

impl StructurePiecePlacer {
    pub(super) fn place_template_piece(
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
        let position = Self::adjusted_template_position(region, &template, data, random);
        let mut hardcoded_processors = Vec::new();
        let processor_list =
            Self::template_processors(registry, &data.processors, &mut hardcoded_processors);
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
                position,
                &settings,
                template_box,
                placement_clip,
            );
        }
        placed
    }

    fn adjusted_template_position(
        region: &WorldGenRegion<'_>,
        template: &StructureTemplate,
        data: &mut TemplatePieceData,
        random: &mut WorldgenRandom,
    ) -> BlockPos {
        match &mut data.placement_adjustment {
            TemplatePlacementAdjustment::None => BlockPos::new(
                data.template_position.0,
                data.template_position.1,
                data.template_position.2,
            ),
            TemplatePlacementAdjustment::Shipwreck {
                is_beached,
                height_adjusted,
            } => {
                if !*height_adjusted && !Self::shipwreck_is_too_big_to_fit(template) {
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
                BlockPos::new(
                    data.template_position.0,
                    data.template_position.1,
                    data.template_position.2,
                )
            }
            TemplatePlacementAdjustment::Igloo { template_offset } => {
                Self::adjusted_igloo_position(
                    region,
                    data.template_position,
                    data.mirror,
                    data.rotation,
                    BlockPos::new(
                        data.rotation_pivot.0,
                        data.rotation_pivot.1,
                        data.rotation_pivot.2,
                    ),
                    *template_offset,
                )
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

    fn adjusted_igloo_position(
        region: &WorldGenRegion<'_>,
        position: (i32, i32, i32),
        mirror: StructureMirror,
        rotation: Rotation,
        pivot: BlockPos,
        template_offset: (i32, i32, i32),
    ) -> BlockPos {
        const IGLOO_GENERATION_HEIGHT: i32 = 90;

        let raw_position = BlockPos::new(position.0, position.1, position.2);
        let entrance_relative = StructureTemplate::calculate_relative_position(
            BlockPos::new(3 - template_offset.0, 0, -template_offset.2),
            mirror,
            rotation,
            pivot,
        );
        let entrance_pos = raw_position.offset(
            entrance_relative.x(),
            entrance_relative.y(),
            entrance_relative.z(),
        );
        let height = region.height_at(
            HeightmapType::WorldSurfaceWg,
            entrance_pos.x(),
            entrance_pos.z(),
        );
        raw_position.offset(0, height - IGLOO_GENERATION_HEIGHT - 1, 0)
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
            TemplateMarkerHandling::Igloo => {
                for marker in template.data_markers(registry, position, settings, random) {
                    Self::handle_igloo_marker(region, &marker, random);
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

    fn handle_igloo_marker(
        region: &mut WorldGenRegion<'_>,
        marker: &StructureDataMarker,
        random: &mut WorldgenRandom,
    ) {
        if marker.metadata != "chest" {
            return;
        }

        let _ = region.set_block_state(
            marker.pos,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_ALL,
        );
        let chest_pos = marker.pos.below();
        let state = region.block_state(chest_pos);
        if state.get_block() != &vanilla_blocks::CHEST {
            return;
        }

        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", "minecraft:chests/igloo_chest");
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
        position: BlockPos,
        settings: &StructurePlaceSettings<'_>,
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
            TemplatePostProcess::IglooTop => {
                Self::post_process_igloo_top(region, position, settings);
            }
        }
    }

    fn post_process_igloo_top(
        region: &mut WorldGenRegion<'_>,
        position: BlockPos,
        settings: &StructurePlaceSettings<'_>,
    ) {
        let trapdoor_relative = StructureTemplate::calculate_relative_position(
            BlockPos::new(3, 0, 5),
            settings.mirror,
            settings.rotation,
            settings.rotation_pivot,
        );
        let trapdoor_pos = position.offset(
            trapdoor_relative.x(),
            trapdoor_relative.y(),
            trapdoor_relative.z(),
        );
        let below_state = region.block_state(trapdoor_pos.below());
        if below_state.is_air() || below_state.get_block() == &vanilla_blocks::LADDER {
            return;
        }

        let _ = region.set_block_state(
            trapdoor_pos,
            vanilla_blocks::SNOW_BLOCK.default_state(),
            UpdateFlags::UPDATE_ALL,
        );
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

    fn dried_ghast_state(registry: &Registry, rotation: Rotation) -> BlockStateId {
        let facing = rotation.rotate(Direction::North);
        let Some(state) = registry.blocks.state_id_from_block_defaulted_properties(
            &vanilla_blocks::DRIED_GHAST,
            [("facing", facing.as_str())],
        ) else {
            panic!("dried_ghast missing vanilla facing property");
        };
        state
    }
}
