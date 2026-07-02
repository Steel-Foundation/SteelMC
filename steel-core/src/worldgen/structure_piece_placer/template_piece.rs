use std::sync::Arc;

use glam::{DVec3, IVec3};
use simdnbt::owned::NbtCompound;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{
    Registry, vanilla_block_entity_types, vanilla_blocks, vanilla_entities, vanilla_items,
};
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::random::{PositionalRandom, Random};
use steel_utils::{BlockPos, BlockStateId, BoundingBox, Direction, Rotation, types::UpdateFlags};

use crate::behavior::BlockStateBehaviorExt;
use crate::chunk::heightmap::HeightmapType;
use crate::entity::{
    entities::{ItemFrameEntity, RawEntity},
    next_entity_id,
};
use crate::fluid::FluidStateExt as _;
use crate::worldgen::region::WorldGenRegion;
use crate::worldgen::template::{
    StructureDataMarker, StructurePlaceSettings, StructureProcessorRandom, StructureTemplate,
};
use steel_worldgen::structure::{
    StructureMirror, TemplateMarkerHandling, TemplatePieceData, TemplatePlacementAdjustment,
    TemplatePlacementClip, TemplatePostProcess, TemplateProcessorList,
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
            rotation_pivot: BlockPos(data.rotation_pivot),
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
        let Some(placement_clip) =
            Self::template_placement_clip(data.placement_clip, clip, template_box)
        else {
            return false;
        };
        let settings = StructurePlaceSettings {
            bounding_box: placement_clip,
            ..settings
        };
        if !template_box.intersects(placement_clip) {
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
                &data.processors,
                position,
                &settings,
                template_box,
                placement_clip,
                random,
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
            TemplatePlacementAdjustment::None => BlockPos(data.template_position),
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
                    data.template_position.y = new_y;
                    *height_adjusted = true;
                }
                BlockPos(data.template_position)
            }
            TemplatePlacementAdjustment::Igloo { template_offset } => {
                Self::adjusted_igloo_position(
                    region,
                    data.template_position,
                    data.mirror,
                    data.rotation,
                    BlockPos(data.rotation_pivot),
                    IVec3::new(template_offset.0, template_offset.1, template_offset.2),
                )
            }
            TemplatePlacementAdjustment::OceanRuin => {
                Self::adjusted_ocean_ruin_position(region, template, data)
            }
        }
    }

    const fn shipwreck_is_too_big_to_fit(template: &StructureTemplate) -> bool {
        let size = template.size(Rotation::None);
        size.x > 32 || size.y > 32
    }

    fn adjusted_shipwreck_y(
        region: &WorldGenRegion<'_>,
        template: &StructureTemplate,
        position: IVec3,
        is_beached: bool,
        random: &mut WorldgenRandom,
    ) -> i32 {
        let size = template.size(Rotation::None);
        let heightmap_type = if is_beached {
            HeightmapType::WorldSurfaceWg
        } else {
            HeightmapType::OceanFloorWg
        };
        let base_area = size.x * size.z;
        if base_area == 0 {
            return region.height_at(heightmap_type, position.x, position.z);
        }

        let mut min_y = region.max_y_exclusive();
        let mut mean = 0;
        for z in position.z..position.z + size.z {
            for x in position.x..position.x + size.x {
                let height = region.height_at(heightmap_type, x, z);
                mean += height;
                min_y = min_y.min(height);
            }
        }
        mean /= base_area;

        if is_beached {
            min_y - size.y / 2 - random.next_i32_bounded(3)
        } else {
            mean
        }
    }

    fn adjusted_igloo_position(
        region: &WorldGenRegion<'_>,
        position: IVec3,
        mirror: StructureMirror,
        rotation: Rotation,
        pivot: BlockPos,
        template_offset: IVec3,
    ) -> BlockPos {
        const IGLOO_GENERATION_HEIGHT: i32 = 90;

        let raw_position = BlockPos(position);
        let entrance_relative = StructureTemplate::calculate_relative_position(
            BlockPos(IVec3::new(3 - template_offset.x, 0, -template_offset.z)),
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

    fn adjusted_ocean_ruin_position(
        region: &WorldGenRegion<'_>,
        template: &StructureTemplate,
        data: &mut TemplatePieceData,
    ) -> BlockPos {
        let ocean_floor_y = region.height_at(
            HeightmapType::OceanFloorWg,
            data.template_position.x,
            data.template_position.z,
        );
        let base = BlockPos(data.template_position.with_y(ocean_floor_y));
        let size = template.size(Rotation::None);
        let corner_iv = data
            .rotation
            .transform_pos(IVec3::new(size.x - 1, 0, size.z - 1), IVec3::ZERO);
        let corner = base.offset(corner_iv.x, 0, corner_iv.z);
        let y = Self::adjusted_ocean_ruin_height(region, base, corner);
        data.template_position.y = y;
        BlockPos(data.template_position)
    }

    fn adjusted_ocean_ruin_height(
        region: &WorldGenRegion<'_>,
        base: BlockPos,
        corner: BlockPos,
    ) -> i32 {
        let mut new_y = base.y();
        let mut min_y = 512;
        let top_y = new_y - 1;
        let mut area = 0;
        let x0 = base.x().min(corner.x());
        let x1 = base.x().max(corner.x());
        let z0 = base.z().min(corner.z());
        let z1 = base.z().max(corner.z());

        for z in z0..=z1 {
            for x in x0..=x1 {
                let mut floor_y = base.y() - 1;
                let mut pos = BlockPos::new(x, floor_y, z);
                let mut state = region.block_state(pos);
                while (state.is_air()
                    || Self::is_water_state(state)
                    || state.get_block().has_tag(&BlockTag::ICE))
                    && floor_y > region.min_y() + 1
                {
                    floor_y -= 1;
                    pos = BlockPos::new(x, floor_y, z);
                    state = region.block_state(pos);
                }

                min_y = min_y.min(floor_y);
                if floor_y < top_y - 2 {
                    area += 1;
                }
            }
        }

        let width = (base.x() - corner.x()).abs();
        if top_y - min_y > 2 && area > width - 2 {
            new_y = min_y + 1;
        }
        new_y
    }

    fn is_water_state(state: BlockStateId) -> bool {
        state.get_fluid_state().is_water()
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
            TemplateMarkerHandling::OceanRuin { is_large } => {
                for marker in template.data_markers(registry, position, settings, random) {
                    Self::handle_ocean_ruin_marker(region, is_large, &marker, random);
                }
                true
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
            TemplateMarkerHandling::EndCity => {
                for marker in template.data_markers(registry, position, settings, random) {
                    Self::handle_end_city_marker(region, settings, &marker, random);
                }
                true
            }
            TemplateMarkerHandling::WoodlandMansion => {
                for marker in template.data_markers(registry, position, settings, random) {
                    Self::handle_mansion_marker(region, settings, &marker, random);
                }
                true
            }
        }
    }

    fn handle_ocean_ruin_marker(
        region: &mut WorldGenRegion<'_>,
        is_large: bool,
        marker: &StructureDataMarker,
        random: &mut WorldgenRandom,
    ) {
        match marker.metadata.as_str() {
            "chest" => Self::place_ocean_ruin_marker_chest(region, is_large, marker.pos, random),
            "drowned" => Self::spawn_ocean_ruin_drowned(region, marker.pos),
            _ => {}
        }
    }

    fn place_ocean_ruin_marker_chest(
        region: &mut WorldGenRegion<'_>,
        is_large: bool,
        pos: BlockPos,
        random: &mut WorldgenRandom,
    ) {
        let waterlogged = Self::is_water_state(region.block_state(pos));
        let state = vanilla_blocks::CHEST
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, waterlogged);
        let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);

        let loot_table = if is_large {
            "minecraft:chests/underwater_ruin_big"
        } else {
            "minecraft:chests/underwater_ruin_small"
        };
        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", loot_table);
        nbt.insert("LootTableSeed", random.next_i64());
        let _ = region.set_block_entity_data(pos, &vanilla_block_entity_types::CHEST, state, nbt);
    }

    fn spawn_ocean_ruin_drowned(region: &mut WorldGenRegion<'_>, pos: BlockPos) {
        let entity_pos = DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()),
            f64::from(pos.z()) + 0.5,
        );
        let entity = Arc::new(RawEntity::new(
            next_entity_id(),
            entity_pos,
            region.weak_world(),
            &vanilla_entities::DROWNED,
        ));
        entity.set_persistence_required();
        entity.snap_to(entity_pos, 0.0, 0.0);
        let _ = region.add_fresh_entity(entity);

        let replacement = if pos.y() > region.sea_level() {
            vanilla_blocks::AIR.default_state()
        } else {
            vanilla_blocks::WATER.default_state()
        };
        let _ = region.set_block_state(pos, replacement, UpdateFlags::UPDATE_CLIENTS);
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

    fn handle_end_city_marker(
        region: &mut WorldGenRegion<'_>,
        settings: &StructurePlaceSettings<'_>,
        marker: &StructureDataMarker,
        random: &mut WorldgenRandom,
    ) {
        if marker.metadata.starts_with("Chest") {
            Self::place_end_city_marker_chest(region, marker.pos.below(), random);
            return;
        }
        if !Self::is_in_spawnable_bounds(marker.pos) {
            return;
        }
        if marker.metadata.starts_with("Sentry") {
            Self::spawn_end_city_shulker(region, marker.pos);
        } else if marker.metadata.starts_with("Elytra") {
            let direction = settings.rotation.rotate(Direction::South);
            Self::spawn_end_city_elytra_frame(region, marker.pos, direction);
        }
    }

    fn place_end_city_marker_chest(
        region: &mut WorldGenRegion<'_>,
        chest_pos: BlockPos,
        random: &mut WorldgenRandom,
    ) {
        let state = region.block_state(chest_pos);
        if state.get_block() != &vanilla_blocks::CHEST {
            return;
        }

        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", "minecraft:chests/end_city_treasure");
        nbt.insert("LootTableSeed", random.next_i64());
        let _ =
            region.set_block_entity_data(chest_pos, &vanilla_block_entity_types::CHEST, state, nbt);
    }

    fn spawn_end_city_shulker(region: &mut WorldGenRegion<'_>, pos: BlockPos) {
        let entity_pos = DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()),
            f64::from(pos.z()) + 0.5,
        );
        let entity = Arc::new(RawEntity::new(
            next_entity_id(),
            entity_pos,
            region.weak_world(),
            &vanilla_entities::SHULKER,
        ));
        entity.snap_to(entity_pos, 0.0, 0.0);
        let _ = region.add_fresh_entity(entity);
    }

    fn spawn_end_city_elytra_frame(
        region: &mut WorldGenRegion<'_>,
        pos: BlockPos,
        direction: Direction,
    ) {
        let entity = Arc::new(ItemFrameEntity::new_attached(
            &vanilla_entities::ITEM_FRAME,
            next_entity_id(),
            pos,
            direction,
            region.weak_world(),
        ));
        entity.set_item(ItemStack::new(&vanilla_items::ITEMS.elytra));
        let _ = region.add_fresh_entity(entity);
    }

    fn handle_mansion_marker(
        region: &mut WorldGenRegion<'_>,
        settings: &StructurePlaceSettings<'_>,
        marker: &StructureDataMarker,
        random: &mut WorldgenRandom,
    ) {
        if marker.metadata.starts_with("Chest") {
            let state =
                Self::mansion_marker_chest_state(settings.rotation, marker.metadata.as_str());
            Self::place_mansion_marker_chest(region, marker.pos, state, random);
            return;
        }

        let (entity_type, count) = match marker.metadata.as_str() {
            "Mage" => (&vanilla_entities::EVOKER, 1),
            "Warrior" => (&vanilla_entities::VINDICATOR, 1),
            "Group of Allays" => (
                &vanilla_entities::ALLAY,
                region.random_mut().next_i32_bounded(3) + 1,
            ),
            _ => return,
        };
        for _ in 0..count {
            Self::spawn_mansion_marker_mob(region, marker.pos, entity_type);
        }
        let _ = region.set_block_state(
            marker.pos,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }

    fn mansion_marker_chest_state(rotation: Rotation, marker: &str) -> BlockStateId {
        let facing = match marker {
            "ChestWest" => Some(rotation.rotate(Direction::West)),
            "ChestEast" => Some(rotation.rotate(Direction::East)),
            "ChestSouth" => Some(rotation.rotate(Direction::South)),
            "ChestNorth" => Some(rotation.rotate(Direction::North)),
            _ => None,
        };
        let state = vanilla_blocks::CHEST.default_state();
        if let Some(facing) = facing {
            state.set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
        } else {
            state
        }
    }

    fn place_mansion_marker_chest(
        region: &mut WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
        random: &mut WorldgenRandom,
    ) {
        if region.block_state(pos).get_block() == &vanilla_blocks::CHEST {
            return;
        }
        if !region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS) {
            return;
        }

        let _ = region.set_block_entity_data(
            pos,
            &vanilla_block_entity_types::CHEST,
            state,
            Self::loot_table_nbt("minecraft:chests/woodland_mansion", random.next_i64()),
        );
    }

    fn spawn_mansion_marker_mob(
        region: &mut WorldGenRegion<'_>,
        pos: BlockPos,
        entity_type: EntityTypeRef,
    ) {
        let entity_pos = DVec3::new(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()));
        let entity = Arc::new(RawEntity::new(
            next_entity_id(),
            entity_pos,
            region.weak_world(),
            entity_type,
        ));
        entity.set_persistence_required();
        entity.snap_to(entity_pos, 0.0, 0.0);
        let _ = region.add_fresh_entity(entity);
    }

    fn loot_table_nbt(loot_table: &'static str, seed: i64) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", loot_table);
        nbt.insert("LootTableSeed", seed);
        nbt
    }

    const fn is_in_spawnable_bounds(pos: BlockPos) -> bool {
        pos.y() >= -20_000_000
            && pos.y() < 20_000_000
            && pos.x() >= -30_000_000
            && pos.z() >= -30_000_000
            && pos.x() < 30_000_000
            && pos.z() < 30_000_000
    }

    fn template_placement_clip(
        placement_clip: TemplatePlacementClip,
        center_clip: BoundingBox,
        template_box: BoundingBox,
    ) -> Option<BoundingBox> {
        match placement_clip {
            TemplatePlacementClip::CenterChunk => Some(center_clip),
            TemplatePlacementClip::CenterChunkExpandedToTemplate => {
                Some(BoundingBox::encapsulating(&center_clip, &template_box))
            }
            TemplatePlacementClip::CenterChunkContainsTemplateCenterExpandedToTemplate => {
                if center_clip.contains(template_box.center()) {
                    Some(BoundingBox::encapsulating(&center_clip, &template_box))
                } else {
                    None
                }
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "postprocess needs the same placement context as vanilla TemplateStructurePiece after block placement"
    )]
    fn post_process_template_piece(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        post_process: TemplatePostProcess,
        processors: &TemplateProcessorList,
        position: BlockPos,
        settings: &StructurePlaceSettings<'_>,
        template_box: BoundingBox,
        placement_clip: BoundingBox,
        random: &mut WorldgenRandom,
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
            TemplatePostProcess::RuinedPortal => {
                let TemplateProcessorList::RuinedPortal {
                    vertical_placement,
                    properties,
                } = processors
                else {
                    panic!("ruined portal postprocess requires ruined portal processors");
                };
                Self::post_process_ruined_portal(
                    region,
                    *vertical_placement,
                    *properties,
                    template_box,
                    random,
                );
            }
        }
    }

    fn post_process_igloo_top(
        region: &mut WorldGenRegion<'_>,
        position: BlockPos,
        settings: &StructurePlaceSettings<'_>,
    ) {
        let trapdoor_relative = StructureTemplate::calculate_relative_position(
            BlockPos(IVec3::new(3, 0, 5)),
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
        let center = fossil_box.center();
        let mut seed_random = LegacyRandom::from_seed(region.seed() as u64);
        let splitter = seed_random.next_positional();
        let mut positional_random = splitter.at(center.x, center.y, center.z);
        if positional_random.next_f32() >= 0.5 {
            return;
        }

        let pos = BlockPos::new(
            fossil_box.min_x() + positional_random.next_i32_bounded(fossil_box.width()),
            fossil_box.min_y(),
            fossil_box.min_z() + positional_random.next_i32_bounded(fossil_box.depth()),
        );
        if !placement_clip.contains_blockpos(pos) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::init_behaviors;
    use steel_registry::test_support::init_test_registry;

    #[test]
    fn center_gated_expanded_clip_requires_template_center_inside_center_chunk() {
        let center_clip = BoundingBox::new(IVec3::new(0, -64, 0), IVec3::new(15, 319, 15));
        let centered_template = BoundingBox::new(IVec3::new(0, 70, 0), IVec3::new(15, 80, 15));
        let outside_template = BoundingBox::new(IVec3::new(16, 70, 8), IVec3::new(31, 80, 23));

        assert_eq!(
            StructurePiecePlacer::template_placement_clip(
                TemplatePlacementClip::CenterChunkContainsTemplateCenterExpandedToTemplate,
                center_clip,
                centered_template,
            ),
            Some(BoundingBox::encapsulating(&center_clip, &centered_template)),
        );
        assert_eq!(
            StructurePiecePlacer::template_placement_clip(
                TemplatePlacementClip::CenterChunkContainsTemplateCenterExpandedToTemplate,
                center_clip,
                outside_template,
            ),
            None,
        );
    }

    #[test]
    fn marker_water_check_uses_fluid_state_for_seagrass() {
        init_test_registry();
        init_behaviors();

        assert!(StructurePiecePlacer::is_water_state(
            vanilla_blocks::SEAGRASS.default_state()
        ));
    }
}
