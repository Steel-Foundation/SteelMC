use crate::worldgen::template::{
    StructurePlaceSettings, StructureProcessorRandom, StructureTemplate,
};
use steel_utils::{BoundingBox, Rotation};
use steel_worldgen::structure::{StructureBlockIgnore, StructureMirror};

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;
use steel_registry::structure::LiquidSettingsData;
use steel_registry::structure_processor::StructureProcessorKind;

const VANILLA_ROTATIONS: [Rotation; 4] = [
    Rotation::None,
    Rotation::Clockwise90,
    Rotation::Clockwise180,
    Rotation::CounterClockwise90,
];

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_fossil_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &FossilConfiguration,
        origin: BlockPos,
    ) -> bool {
        assert!(
            !(config.fossil_structures.is_empty() || config.overlay_structures.is_empty()),
            "fossil feature must have at least one base and overlay structure"
        );
        assert!(
            config.fossil_structures.len() == config.overlay_structures.len(),
            "fossil feature has {} base structures but {} overlay structures",
            config.fossil_structures.len(),
            config.overlay_structures.len()
        );

        let rotation = VANILLA_ROTATIONS[random.next_i32_bounded(4) as usize];
        let structure_count = config.fossil_structures.len();
        let Ok(structure_count_i32) = i32::try_from(structure_count) else {
            panic!("fossil structure count {structure_count} exceeds i32 range");
        };
        let structure_index = random.next_i32_bounded(structure_count_i32) as usize;

        let fossil_template =
            Self::load_fossil_template(registry, &config.fossil_structures[structure_index]);
        let overlay_template =
            Self::load_fossil_template(registry, &config.overlay_structures[structure_index]);

        let size = fossil_template.size(rotation);
        let low_corner = origin.offset(-size[0] / 2, 0, -size[2] / 2);
        let lowest_surface_y =
            Self::lowest_fossil_surface_y(region, low_corner, size[0], size[2], origin.y());
        let target_y =
            (lowest_surface_y - 15 - random.next_i32_bounded(10)).max(region.min_y() + 10);
        let target_pos =
            fossil_template.zero_position_with_transform(low_corner.at_y(target_y), rotation);
        let template_box = fossil_template.bounding_box(target_pos, rotation);

        if Self::count_empty_corners(region, template_box) > config.max_empty_corners_allowed {
            return false;
        }

        let bounding_box = Self::fossil_bounding_box(region, origin);
        let fossil_processors =
            Self::structure_processors(registry, &config.fossil_processors, "fossil processors");
        let overlay_processors = Self::structure_processors(
            registry,
            &config.overlay_processors,
            "fossil overlay processors",
        );

        let fossil_settings = StructurePlaceSettings {
            mirror: StructureMirror::None,
            rotation,
            rotation_pivot: BlockPos::ZERO,
            bounding_box,
            processors: fossil_processors,
            block_ignore: StructureBlockIgnore::None,
            late_block_ignore: StructureBlockIgnore::None,
            replace_jigsaws: false,
            projection: None,
            processor_random: StructureProcessorRandom::Placement,
            liquid_settings: LiquidSettingsData::ApplyWaterlogging,
        };
        fossil_template.place_in_world(
            region,
            registry,
            target_pos,
            target_pos,
            &fossil_settings,
            random,
            UpdateFlags::UPDATE_NONE,
        );

        let overlay_settings = StructurePlaceSettings {
            mirror: StructureMirror::None,
            rotation,
            rotation_pivot: BlockPos::ZERO,
            bounding_box,
            processors: overlay_processors,
            block_ignore: StructureBlockIgnore::None,
            late_block_ignore: StructureBlockIgnore::None,
            replace_jigsaws: false,
            projection: None,
            processor_random: StructureProcessorRandom::Placement,
            liquid_settings: LiquidSettingsData::ApplyWaterlogging,
        };
        overlay_template.place_in_world(
            region,
            registry,
            target_pos,
            target_pos,
            &overlay_settings,
            random,
            UpdateFlags::UPDATE_NONE,
        );

        true
    }

    fn load_fossil_template(registry: &Registry, key: &Identifier) -> StructureTemplate {
        match StructureTemplate::load_vanilla(registry, key) {
            Ok(template) => template,
            Err(err) => panic!("{err}"),
        }
    }

    fn structure_processors<'a>(
        registry: &'a Registry,
        key: &Identifier,
        context: &str,
    ) -> &'a [StructureProcessorKind] {
        let Some(processor_list) = registry.structure_processors.by_key(key) else {
            panic!("{context} references unknown processor list {key}");
        };
        &processor_list.data.processors
    }

    fn lowest_fossil_surface_y(
        region: &WorldGenRegion<'_>,
        low_corner: BlockPos,
        size_x: i32,
        size_z: i32,
        initial_y: i32,
    ) -> i32 {
        let mut lowest = initial_y;
        for dx in 0..size_x {
            for dz in 0..size_z {
                let y = region.height_at(
                    HeightmapType::OceanFloorWg,
                    low_corner.x() + dx,
                    low_corner.z() + dz,
                );
                lowest = lowest.min(y);
            }
        }
        lowest
    }

    const fn fossil_bounding_box(region: &WorldGenRegion<'_>, origin: BlockPos) -> BoundingBox {
        let chunk_x = SectionPos::block_to_section_coord(origin.x());
        let chunk_z = SectionPos::block_to_section_coord(origin.z());
        let min_x = chunk_x << 4;
        let min_z = chunk_z << 4;
        BoundingBox::new(
            min_x - 16,
            region.min_y(),
            min_z - 16,
            min_x + 15 + 16,
            region.max_y_exclusive() - 1,
            min_z + 15 + 16,
        )
    }

    fn count_empty_corners(region: &WorldGenRegion<'_>, bounding_box: BoundingBox) -> i32 {
        let mut count = 0;
        for x in [bounding_box.min_x, bounding_box.max_x] {
            for y in [bounding_box.min_y, bounding_box.max_y] {
                for z in [bounding_box.min_z, bounding_box.max_z] {
                    let state = region.block_state(BlockPos::new(x, y, z));
                    let block = state.get_block();
                    if state.is_air()
                        || block == &vanilla_blocks::LAVA
                        || block == &vanilla_blocks::WATER
                    {
                        count += 1;
                    }
                }
            }
        }
        count
    }
}
