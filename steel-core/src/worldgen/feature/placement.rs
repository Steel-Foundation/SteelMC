use super::prelude::*;
use super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    #[expect(
        clippy::too_many_lines,
        reason = "keeps the vanilla placement modifier dispatch table in one place"
    )]
    pub(super) fn apply_placement_modifier(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        biome_filter_feature_id: Option<usize>,
        modifier: &PlacementModifier,
        biome_zoom_seed: i64,
    ) -> Vec<BlockPos> {
        match modifier {
            PlacementModifier::Biome => {
                if Self::biome_allows_feature(
                    region,
                    registry,
                    biome_zoom_seed,
                    origin,
                    biome_filter_feature_id,
                ) {
                    vec![origin]
                } else {
                    Vec::new()
                }
            }
            PlacementModifier::BlockPredicateFilter { predicate } => {
                if Self::test_block_predicate(region, registry, predicate, origin) {
                    vec![origin]
                } else {
                    Vec::new()
                }
            }
            PlacementModifier::Count { count } => {
                Self::repeat_position(origin, count.sample(random))
            }
            PlacementModifier::CountOnEveryLayer { count } => {
                Self::count_on_every_layer_positions(region, random, origin, count)
            }
            PlacementModifier::EnvironmentScan {
                direction_of_search,
                target_condition,
                allowed_search_condition,
                max_steps,
            } => Self::environment_scan_positions(
                region,
                registry,
                origin,
                *direction_of_search,
                target_condition,
                allowed_search_condition.as_ref(),
                *max_steps,
            ),
            PlacementModifier::FixedPlacement { positions } => {
                let chunk_x = SectionPos::block_to_section_coord(origin.x());
                let chunk_z = SectionPos::block_to_section_coord(origin.z());
                positions
                    .iter()
                    .map(|position| BlockPos::new(position[0], position[1], position[2]))
                    .filter(|position| {
                        chunk_x == SectionPos::block_to_section_coord(position.x())
                            && chunk_z == SectionPos::block_to_section_coord(position.z())
                    })
                    .collect()
            }
            PlacementModifier::HeightRange { height } => {
                vec![BlockPos::new(
                    origin.x(),
                    height.sample(
                        random,
                        region.generation_min_y(),
                        region.generation_height(),
                    ),
                    origin.z(),
                )]
            }
            PlacementModifier::Heightmap { heightmap } => {
                let height = region.height_at(
                    Self::feature_heightmap_type(*heightmap),
                    origin.x(),
                    origin.z(),
                );
                if height > region.min_y() {
                    vec![BlockPos::new(origin.x(), height, origin.z())]
                } else {
                    Vec::new()
                }
            }
            PlacementModifier::InSquare => {
                let x = origin.x() + random.next_i32_bounded(16);
                let z = origin.z() + random.next_i32_bounded(16);
                vec![BlockPos::new(x, origin.y(), z)]
            }
            PlacementModifier::NoiseBasedCount {
                noise_to_count_ratio,
                noise_factor,
                noise_offset,
            } => {
                let noise = Self::biome_info_noise_value(
                    f64::from(origin.x()) / *noise_factor,
                    f64::from(origin.z()) / *noise_factor,
                );
                let count =
                    ((noise + *noise_offset) * f64::from(*noise_to_count_ratio)).ceil() as i32;
                Self::repeat_position(origin, count)
            }
            PlacementModifier::NoiseThresholdCount {
                noise_level,
                below_noise,
                above_noise,
            } => {
                let noise = Self::biome_info_noise_value(
                    f64::from(origin.x()) / 200.0,
                    f64::from(origin.z()) / 200.0,
                );
                let count = if noise < *noise_level {
                    *below_noise
                } else {
                    *above_noise
                };
                Self::repeat_position(origin, count)
            }
            PlacementModifier::RandomOffset {
                xz_spread,
                y_spread,
            } => {
                let x_offset = xz_spread.sample(random);
                let y_offset = y_spread.sample(random);
                let z_offset = xz_spread.sample(random);
                vec![BlockPos::new(
                    origin.x() + x_offset,
                    origin.y() + y_offset,
                    origin.z() + z_offset,
                )]
            }
            PlacementModifier::RarityFilter { chance } => {
                assert!(
                    *chance > 0,
                    "rarity filter chance must be positive, got {chance}"
                );
                if random.next_f32() < 1.0 / (*chance as f32) {
                    vec![origin]
                } else {
                    Vec::new()
                }
            }
            PlacementModifier::SurfaceRelativeThresholdFilter {
                heightmap,
                min_inclusive,
                max_inclusive,
            } => {
                let surface_y = i64::from(region.height_at(
                    Self::feature_heightmap_type(*heightmap),
                    origin.x(),
                    origin.z(),
                ));
                let min_y = surface_y + i64::from(min_inclusive.unwrap_or(i32::MIN));
                let max_y = surface_y + i64::from(max_inclusive.unwrap_or(i32::MAX));
                let origin_y = i64::from(origin.y());
                if min_y <= origin_y && origin_y <= max_y {
                    vec![origin]
                } else {
                    Vec::new()
                }
            }
            PlacementModifier::SurfaceWaterDepthFilter { max_water_depth } => {
                let ocean_floor =
                    region.height_at(HeightmapType::OceanFloor, origin.x(), origin.z());
                let surface = region.height_at(HeightmapType::WorldSurface, origin.x(), origin.z());
                if surface - ocean_floor <= *max_water_depth {
                    vec![origin]
                } else {
                    Vec::new()
                }
            }
        }
    }

    pub(super) const fn feature_heightmap_type(heightmap: FeatureHeightmap) -> HeightmapType {
        match heightmap {
            FeatureHeightmap::WorldSurface => HeightmapType::WorldSurface,
            FeatureHeightmap::MotionBlocking => HeightmapType::MotionBlocking,
            FeatureHeightmap::MotionBlockingNoLeaves => HeightmapType::MotionBlockingNoLeaves,
            FeatureHeightmap::OceanFloor => HeightmapType::OceanFloor,
            FeatureHeightmap::WorldSurfaceWg => HeightmapType::WorldSurfaceWg,
            FeatureHeightmap::OceanFloorWg => HeightmapType::OceanFloorWg,
        }
    }

    pub(super) fn repeat_position(origin: BlockPos, count: i32) -> Vec<BlockPos> {
        let Ok(count) = usize::try_from(count) else {
            return Vec::new();
        };
        vec![origin; count]
    }

    pub(super) fn count_on_every_layer_positions(
        region: &WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        count: &IntProvider,
    ) -> Vec<BlockPos> {
        let mut positions = Vec::new();
        let mut layer = 0;

        loop {
            let mut found_any = false;
            for _ in 0..count.sample(random) {
                let x = origin.x() + random.next_i32_bounded(16);
                let z = origin.z() + random.next_i32_bounded(16);
                let start_y = region.height_at(HeightmapType::MotionBlocking, x, z);
                if let Some(y) = Self::find_on_ground_y_position(region, x, start_y, z, layer) {
                    positions.push(BlockPos::new(x, y, z));
                    found_any = true;
                }
            }

            if !found_any {
                break;
            }
            layer += 1;
        }

        positions
    }

    pub(super) fn find_on_ground_y_position(
        region: &WorldGenRegion<'_>,
        x: i32,
        start_y: i32,
        z: i32,
        layer_to_place_on: i32,
    ) -> Option<i32> {
        let mut current_layer = 0;
        let mut current_block = region.block_state(BlockPos::new(x, start_y, z));

        for y in (region.min_y() + 1..=start_y).rev() {
            let below_block = region.block_state(BlockPos::new(x, y - 1, z));
            if !Self::is_empty_layer_block(below_block)
                && Self::is_empty_layer_block(current_block)
                && below_block.get_block() != &vanilla_blocks::BEDROCK
            {
                if current_layer == layer_to_place_on {
                    return Some(y);
                }
                current_layer += 1;
            }

            current_block = below_block;
        }

        None
    }

    pub(super) fn is_empty_layer_block(state: steel_utils::BlockStateId) -> bool {
        state.is_air()
            || state.get_block() == &vanilla_blocks::WATER
            || state.get_block() == &vanilla_blocks::LAVA
    }

    pub(super) fn environment_scan_positions(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        origin: BlockPos,
        direction_of_search: steel_utils::Direction,
        target_condition: &BlockPredicate,
        allowed_search_condition: Option<&BlockPredicate>,
        max_steps: i32,
    ) -> Vec<BlockPos> {
        assert!(
            max_steps > 0,
            "environment scan max_steps must be positive, got {max_steps}"
        );

        let mut position = origin;
        if !Self::test_optional_block_predicate(
            region,
            registry,
            allowed_search_condition,
            position,
        ) {
            return Vec::new();
        }

        for _ in 0..max_steps {
            if Self::test_block_predicate(region, registry, target_condition, position) {
                return vec![position];
            }

            position = position.relative(direction_of_search);
            if region.is_outside_build_height(position.y()) {
                return Vec::new();
            }

            if !Self::test_optional_block_predicate(
                region,
                registry,
                allowed_search_condition,
                position,
            ) {
                break;
            }
        }

        if Self::test_block_predicate(region, registry, target_condition, position) {
            vec![position]
        } else {
            Vec::new()
        }
    }
}
