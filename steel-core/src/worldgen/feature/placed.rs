#![expect(
    clippy::too_many_lines,
    reason = "placed feature modifier chaining mirrors vanilla placement flow"
)]

use super::prelude::*;
use super::runner::FeatureDecorationRunner;

#[derive(Clone, Copy)]
enum BiomeFilterMode<'a> {
    Check(Option<&'a Identifier>),
    Ignore,
}

impl FeatureDecorationRunner {
    pub(super) fn place_placed_feature_entry(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        feature: PlacedFeatureEntryRef,
        biome_zoom_seed: i64,
    ) -> bool {
        assert!(
            feature.try_id().is_some(),
            "top-level placed feature {} is not registered",
            feature.key
        );
        Self::place_placed_feature_data(
            region,
            registry,
            random,
            origin,
            &feature.data,
            Some(&feature.key),
            biome_zoom_seed,
        )
    }

    pub(super) fn place_placed_feature_data(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        feature: &PlacedFeatureData,
        biome_filter: Option<&Identifier>,
        biome_zoom_seed: i64,
    ) -> bool {
        Self::place_placed_feature_from_modifier(
            region,
            registry,
            random,
            origin,
            feature,
            BiomeFilterMode::Check(biome_filter),
            biome_zoom_seed,
            0,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "threading vanilla placed-feature stream state explicitly"
    )]
    fn place_placed_feature_from_modifier(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        feature: &PlacedFeatureData,
        biome_filter: BiomeFilterMode<'_>,
        biome_zoom_seed: i64,
        modifier_index: usize,
    ) -> bool {
        let Some(modifier) = feature.placement.get(modifier_index) else {
            return Self::place_configured_feature(
                region,
                registry,
                random,
                &feature.feature,
                origin,
                biome_zoom_seed,
            );
        };

        let mut placed = false;

        match modifier {
            PlacementModifier::Biome => {
                let biome_allows = match biome_filter {
                    BiomeFilterMode::Check(feature_key) => Self::biome_allows_feature(
                        region,
                        registry,
                        biome_zoom_seed,
                        origin,
                        feature_key,
                    ),
                    BiomeFilterMode::Ignore => true,
                };

                if biome_allows {
                    placed = Self::place_placed_feature_from_modifier(
                        region,
                        registry,
                        random,
                        origin,
                        feature,
                        biome_filter,
                        biome_zoom_seed,
                        modifier_index + 1,
                    );
                }
            }
            PlacementModifier::BlockPredicateFilter { predicate } => {
                if Self::test_block_predicate(region, registry, predicate, origin) {
                    placed = Self::place_placed_feature_from_modifier(
                        region,
                        registry,
                        random,
                        origin,
                        feature,
                        biome_filter,
                        biome_zoom_seed,
                        modifier_index + 1,
                    );
                }
            }
            PlacementModifier::Count { count } => {
                if let Ok(count) = usize::try_from(count.sample(random)) {
                    for _ in 0..count {
                        if Self::place_placed_feature_from_modifier(
                            region,
                            registry,
                            random,
                            origin,
                            feature,
                            biome_filter,
                            biome_zoom_seed,
                            modifier_index + 1,
                        ) {
                            placed = true;
                        }
                    }
                }
            }
            PlacementModifier::CountOnEveryLayer { count } => {
                for position in Self::count_on_every_layer_positions(region, random, origin, count)
                {
                    if Self::place_placed_feature_from_modifier(
                        region,
                        registry,
                        random,
                        position,
                        feature,
                        biome_filter,
                        biome_zoom_seed,
                        modifier_index + 1,
                    ) {
                        placed = true;
                    }
                }
            }
            PlacementModifier::EnvironmentScan {
                direction_of_search,
                target_condition,
                allowed_search_condition,
                max_steps,
            } => {
                if let Some(position) = Self::environment_scan_position(
                    region,
                    registry,
                    origin,
                    *direction_of_search,
                    target_condition,
                    allowed_search_condition.as_ref(),
                    *max_steps,
                ) {
                    placed = Self::place_placed_feature_from_modifier(
                        region,
                        registry,
                        random,
                        position,
                        feature,
                        biome_filter,
                        biome_zoom_seed,
                        modifier_index + 1,
                    );
                }
            }
            PlacementModifier::FixedPlacement { positions } => {
                let chunk_x = SectionPos::block_to_section_coord(origin.x());
                let chunk_z = SectionPos::block_to_section_coord(origin.z());
                for position in positions {
                    let position = BlockPos::new(position[0], position[1], position[2]);
                    if chunk_x != SectionPos::block_to_section_coord(position.x())
                        || chunk_z != SectionPos::block_to_section_coord(position.z())
                    {
                        continue;
                    }
                    if Self::place_placed_feature_from_modifier(
                        region,
                        registry,
                        random,
                        position,
                        feature,
                        biome_filter,
                        biome_zoom_seed,
                        modifier_index + 1,
                    ) {
                        placed = true;
                    }
                }
            }
            PlacementModifier::HeightRange { height } => {
                let position = BlockPos::new(
                    origin.x(),
                    height.sample(
                        random,
                        region.generation_min_y(),
                        region.generation_height(),
                    ),
                    origin.z(),
                );
                placed = Self::place_placed_feature_from_modifier(
                    region,
                    registry,
                    random,
                    position,
                    feature,
                    biome_filter,
                    biome_zoom_seed,
                    modifier_index + 1,
                );
            }
            PlacementModifier::Heightmap { heightmap } => {
                let height = region.height_at(
                    Self::feature_heightmap_type(*heightmap),
                    origin.x(),
                    origin.z(),
                );
                if height > region.min_y() {
                    placed = Self::place_placed_feature_from_modifier(
                        region,
                        registry,
                        random,
                        BlockPos::new(origin.x(), height, origin.z()),
                        feature,
                        biome_filter,
                        biome_zoom_seed,
                        modifier_index + 1,
                    );
                }
            }
            PlacementModifier::InSquare => {
                let position = BlockPos::new(
                    origin.x() + random.next_i32_bounded(16),
                    origin.y(),
                    origin.z() + random.next_i32_bounded(16),
                );
                placed = Self::place_placed_feature_from_modifier(
                    region,
                    registry,
                    random,
                    position,
                    feature,
                    biome_filter,
                    biome_zoom_seed,
                    modifier_index + 1,
                );
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
                if let Ok(count) = usize::try_from(count) {
                    for _ in 0..count {
                        if Self::place_placed_feature_from_modifier(
                            region,
                            registry,
                            random,
                            origin,
                            feature,
                            biome_filter,
                            biome_zoom_seed,
                            modifier_index + 1,
                        ) {
                            placed = true;
                        }
                    }
                }
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
                if let Ok(count) = usize::try_from(count) {
                    for _ in 0..count {
                        if Self::place_placed_feature_from_modifier(
                            region,
                            registry,
                            random,
                            origin,
                            feature,
                            biome_filter,
                            biome_zoom_seed,
                            modifier_index + 1,
                        ) {
                            placed = true;
                        }
                    }
                }
            }
            PlacementModifier::RandomOffset {
                xz_spread,
                y_spread,
            } => {
                let position = BlockPos::new(
                    origin.x() + xz_spread.sample(random),
                    origin.y() + y_spread.sample(random),
                    origin.z() + xz_spread.sample(random),
                );
                placed = Self::place_placed_feature_from_modifier(
                    region,
                    registry,
                    random,
                    position,
                    feature,
                    biome_filter,
                    biome_zoom_seed,
                    modifier_index + 1,
                );
            }
            PlacementModifier::RarityFilter { chance } => {
                assert!(
                    *chance > 0,
                    "rarity filter chance must be positive, got {chance}"
                );
                if random.next_f32() < 1.0 / (*chance as f32) {
                    placed = Self::place_placed_feature_from_modifier(
                        region,
                        registry,
                        random,
                        origin,
                        feature,
                        biome_filter,
                        biome_zoom_seed,
                        modifier_index + 1,
                    );
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
                    placed = Self::place_placed_feature_from_modifier(
                        region,
                        registry,
                        random,
                        origin,
                        feature,
                        biome_filter,
                        biome_zoom_seed,
                        modifier_index + 1,
                    );
                }
            }
            PlacementModifier::SurfaceWaterDepthFilter { max_water_depth } => {
                let ocean_floor =
                    region.height_at(HeightmapType::OceanFloor, origin.x(), origin.z());
                let surface = region.height_at(HeightmapType::WorldSurface, origin.x(), origin.z());
                if surface - ocean_floor <= *max_water_depth {
                    placed = Self::place_placed_feature_from_modifier(
                        region,
                        registry,
                        random,
                        origin,
                        feature,
                        biome_filter,
                        biome_zoom_seed,
                        modifier_index + 1,
                    );
                }
            }
        }

        placed
    }

    pub(super) fn place_placed_feature_ref(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        feature: &PlacedFeatureRef,
        biome_zoom_seed: i64,
    ) -> bool {
        let feature_data = match feature {
            PlacedFeatureRef::Reference(feature) => &feature.data,
            PlacedFeatureRef::Inline(feature) => feature,
        };

        Self::place_placed_feature_data(
            region,
            registry,
            random,
            origin,
            feature_data,
            None,
            biome_zoom_seed,
        )
    }

    pub(crate) fn place_structure_pool_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        feature_key: &Identifier,
        biome_zoom_seed: i64,
    ) -> bool {
        let Some(feature) = registry.placed_features.by_key(feature_key) else {
            panic!("template pool references unknown placed feature {feature_key}");
        };

        Self::place_placed_feature_from_modifier(
            region,
            registry,
            random,
            origin,
            &feature.data,
            BiomeFilterMode::Ignore,
            biome_zoom_seed,
            0,
        )
    }
}
