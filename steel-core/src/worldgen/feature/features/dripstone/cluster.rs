#![expect(
    clippy::too_many_lines,
    reason = "dripstone cluster placement follows vanilla's linear algorithm"
)]

use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;

#[derive(Clone, Copy)]
pub(in crate::worldgen::feature) struct DripstoneColumn {
    pub(in crate::worldgen::feature) floor: Option<i32>,
    pub(in crate::worldgen::feature) ceiling: Option<i32>,
}

impl DripstoneColumn {
    const fn with_floor(self, floor: Option<i32>) -> Self {
        Self {
            floor,
            ceiling: self.ceiling,
        }
    }

    const fn height(self) -> Option<i32> {
        match (self.floor, self.ceiling) {
            (Some(floor), Some(ceiling)) => Some(ceiling - floor - 1),
            _ => None,
        }
    }
}

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_dripstone_cluster_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &DripstoneClusterConfiguration,
        origin: BlockPos,
    ) -> bool {
        if !Self::is_empty_or_water(region.block_state(origin)) {
            return false;
        }

        let height = config.height.sample(random);
        let wetness = config.wetness.sample(random);
        let density = config.density.sample(random);
        let x_radius = config.radius.sample(random);
        let z_radius = config.radius.sample(random);

        for dx in -x_radius..=x_radius {
            for dz in -z_radius..=z_radius {
                let chance =
                    Self::chance_of_stalagmite_or_stalactite(x_radius, z_radius, dx, dz, config);
                let pos = origin.offset(dx, 0, dz);
                Self::place_dripstone_cluster_column(
                    region, registry, random, pos, dx, dz, wetness, chance, height, density, config,
                );
            }
        }

        true
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla DripstoneClusterFeature.placeColumn state"
    )]
    fn place_dripstone_cluster_column(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        dx: i32,
        dz: i32,
        chance_of_water: f32,
        chance_of_stalagmite_or_stalactite: f64,
        cluster_height: i32,
        density: f32,
        config: &DripstoneClusterConfiguration,
    ) {
        let Some(base_column) = Self::scan_dripstone_column(
            region,
            pos,
            config.floor_to_ceiling_search_range,
            Self::is_empty_or_water,
            Self::is_neither_empty_nor_water,
        ) else {
            return;
        };

        let ceiling = base_column.ceiling;
        let base_floor = base_column.floor;
        if ceiling.is_none() && base_floor.is_none() {
            return;
        }

        let want_pool = random.next_f32() < chance_of_water;
        let column = if let Some(base_floor_y) = base_floor {
            if want_pool && Self::can_place_dripstone_pool(region, registry, pos.at_y(base_floor_y))
            {
                let _ = region.set_block_state(
                    pos.at_y(base_floor_y),
                    vanilla_blocks::WATER.default_state(),
                    UpdateFlags::UPDATE_CLIENTS,
                );
                base_column.with_floor(Some(base_floor_y - 1))
            } else {
                base_column
            }
        } else {
            base_column
        };

        let floor = column.floor;
        let want_stalactite = random.next_f64() < chance_of_stalagmite_or_stalactite;
        let stalactite_height = if let Some(ceiling_y) = ceiling {
            if want_stalactite && !Self::is_lava(region, pos.at_y(ceiling_y)) {
                let ceiling_thickness = config.dripstone_block_layer_thickness.sample(random);
                Self::replace_blocks_with_dripstone_blocks(
                    region,
                    registry,
                    pos.at_y(ceiling_y),
                    ceiling_thickness,
                    Direction::Up,
                );
                let max_height = if let Some(floor_y) = floor {
                    cluster_height.min(ceiling_y - floor_y)
                } else {
                    cluster_height
                };
                Self::dripstone_cluster_height(random, dx, dz, density, max_height, config)
            } else {
                0
            }
        } else {
            0
        };

        let want_stalagmite = random.next_f64() < chance_of_stalagmite_or_stalactite;
        let stalagmite_height = if let Some(floor_y) = floor {
            if want_stalagmite && !Self::is_lava(region, pos.at_y(floor_y)) {
                let floor_thickness = config.dripstone_block_layer_thickness.sample(random);
                Self::replace_blocks_with_dripstone_blocks(
                    region,
                    registry,
                    pos.at_y(floor_y),
                    floor_thickness,
                    Direction::Down,
                );
                if ceiling.is_some() {
                    (stalactite_height
                        + random.next_i32_between(
                            -config.max_stalagmite_stalactite_height_diff,
                            config.max_stalagmite_stalactite_height_diff,
                        ))
                    .max(0)
                } else {
                    Self::dripstone_cluster_height(random, dx, dz, density, cluster_height, config)
                }
            } else {
                0
            }
        } else {
            0
        };

        let (actual_stalactite_height, actual_stalagmite_height) =
            if let (Some(ceiling_y), Some(floor_y)) = (ceiling, floor) {
                if ceiling_y - stalactite_height <= floor_y + stalagmite_height {
                    let lowest_stalactite_bottom = (ceiling_y - stalactite_height).max(floor_y + 1);
                    let highest_stalagmite_top = (floor_y + stalagmite_height).min(ceiling_y - 1);
                    let actual_stalactite_bottom = random
                        .next_i32_between(lowest_stalactite_bottom, highest_stalagmite_top + 1);
                    let actual_stalagmite_top = actual_stalactite_bottom - 1;
                    (
                        ceiling_y - actual_stalactite_bottom,
                        actual_stalagmite_top - floor_y,
                    )
                } else {
                    (stalactite_height, stalagmite_height)
                }
            } else {
                (stalactite_height, stalagmite_height)
            };

        let merge_tips = random.next_bool()
            && actual_stalactite_height > 0
            && actual_stalagmite_height > 0
            && column.height().is_some_and(|height| {
                actual_stalactite_height + actual_stalagmite_height == height
            });

        if let Some(ceiling_y) = ceiling {
            Self::grow_pointed_dripstone(
                region,
                registry,
                pos.at_y(ceiling_y - 1),
                Direction::Down,
                actual_stalactite_height,
                merge_tips,
            );
        }

        if let Some(floor_y) = floor {
            Self::grow_pointed_dripstone(
                region,
                registry,
                pos.at_y(floor_y + 1),
                Direction::Up,
                actual_stalagmite_height,
                merge_tips,
            );
        }
    }

    pub(in crate::worldgen::feature) fn scan_dripstone_column<I, V>(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        search_range: i32,
        inside_column: I,
        valid_edge: V,
    ) -> Option<DripstoneColumn>
    where
        I: Fn(BlockStateId) -> bool,
        V: Fn(BlockStateId) -> bool,
    {
        if !inside_column(region.block_state(pos)) {
            return None;
        }

        let ceiling = Self::scan_dripstone_column_direction(
            region,
            pos,
            search_range,
            &inside_column,
            &valid_edge,
            Direction::Up,
        );
        let floor = Self::scan_dripstone_column_direction(
            region,
            pos,
            search_range,
            &inside_column,
            &valid_edge,
            Direction::Down,
        );

        Some(DripstoneColumn { floor, ceiling })
    }

    fn scan_dripstone_column_direction<I, V>(
        region: &WorldGenRegion<'_>,
        mut pos: BlockPos,
        search_range: i32,
        inside_column: &I,
        valid_edge: &V,
        direction: Direction,
    ) -> Option<i32>
    where
        I: Fn(BlockStateId) -> bool,
        V: Fn(BlockStateId) -> bool,
    {
        for _ in 1..search_range {
            if !inside_column(region.block_state(pos)) {
                break;
            }
            pos = pos.relative(direction);
        }

        valid_edge(region.block_state(pos)).then_some(pos.y())
    }

    fn dripstone_cluster_height(
        random: &mut WorldgenRandom,
        dx: i32,
        dz: i32,
        density: f32,
        max_height: i32,
        config: &DripstoneClusterConfiguration,
    ) -> i32 {
        if random.next_f32() > density {
            return 0;
        }

        let distance_from_center = dx.abs() + dz.abs();
        let height_mean = Self::clamped_map_f64(
            f64::from(distance_from_center),
            0.0,
            f64::from(config.max_distance_from_center_affecting_height_bias),
            f64::from(max_height) / 2.0,
            0.0,
        ) as f32;
        Self::random_between_biased(
            random,
            0.0,
            max_height as f32,
            height_mean,
            config.height_deviation as f32,
        ) as i32
    }

    fn can_place_dripstone_pool(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        pos: BlockPos,
    ) -> bool {
        let state = region.block_state(pos);
        let block = state.get_block();
        if block == &vanilla_blocks::WATER
            || block == &vanilla_blocks::DRIPSTONE_BLOCK
            || block == &vanilla_blocks::POINTED_DRIPSTONE
        {
            return false;
        }

        if get_fluid_state_from_block(region.block_state(pos.above())).is_water() {
            return false;
        }

        for direction in Self::VANILLA_HORIZONTAL_DIRECTIONS {
            if !Self::can_be_adjacent_to_dripstone_pool_water(
                region,
                registry,
                pos.relative(direction),
            ) {
                return false;
            }
        }

        Self::can_be_adjacent_to_dripstone_pool_water(region, registry, pos.below())
    }

    fn can_be_adjacent_to_dripstone_pool_water(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        pos: BlockPos,
    ) -> bool {
        let state = region.block_state(pos);
        registry.blocks.is_in_tag(
            state.get_block(),
            &vanilla_block_tags::BASE_STONE_OVERWORLD_TAG,
        ) || get_fluid_state_from_block(state).is_water()
    }

    fn replace_blocks_with_dripstone_blocks(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        mut pos: BlockPos,
        max_count: i32,
        direction: Direction,
    ) {
        for _ in 0..max_count {
            if !Self::place_dripstone_block_if_possible(region, registry, pos) {
                return;
            }

            pos = pos.relative(direction);
        }
    }

    fn chance_of_stalagmite_or_stalactite(
        x_radius: i32,
        z_radius: i32,
        dx: i32,
        dz: i32,
        config: &DripstoneClusterConfiguration,
    ) -> f64 {
        let x_distance_from_edge = x_radius - dx.abs();
        let z_distance_from_edge = z_radius - dz.abs();
        let distance_from_edge = x_distance_from_edge.min(z_distance_from_edge);
        f64::from(Self::clamped_map_f32(
            distance_from_edge as f32,
            0.0,
            config.max_distance_from_edge_affecting_chance_of_dripstone_column as f32,
            config.chance_of_dripstone_column_at_max_distance_from_center,
            1.0,
        ))
    }

    pub(in crate::worldgen::feature) fn is_neither_empty_nor_water(state: BlockStateId) -> bool {
        !state.is_air() && state.get_block() != &vanilla_blocks::WATER
    }

    pub(in crate::worldgen::feature) fn is_empty_or_water_or_lava(state: BlockStateId) -> bool {
        state.is_air()
            || state.get_block() == &vanilla_blocks::WATER
            || state.get_block() == &vanilla_blocks::LAVA
    }

    pub(in crate::worldgen::feature) fn is_lava(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
    ) -> bool {
        region.block_state(pos).get_block() == &vanilla_blocks::LAVA
    }

    pub(in crate::worldgen::feature) fn clamped_map_f32(
        value: f32,
        old_min: f32,
        old_max: f32,
        new_min: f32,
        new_max: f32,
    ) -> f32 {
        if value <= old_min {
            return new_min;
        }
        if value >= old_max {
            return new_max;
        }

        let factor = (value - old_min) / (old_max - old_min);
        new_min + factor * (new_max - new_min)
    }

    fn clamped_map_f64(value: f64, old_min: f64, old_max: f64, new_min: f64, new_max: f64) -> f64 {
        if value <= old_min {
            return new_min;
        }
        if value >= old_max {
            return new_max;
        }

        let factor = (value - old_min) / (old_max - old_min);
        new_min + factor * (new_max - new_min)
    }

    fn random_between_biased(
        random: &mut WorldgenRandom,
        min: f32,
        max: f32,
        mean: f32,
        deviation: f32,
    ) -> f32 {
        let sample = mean + deviation * random.next_gaussian() as f32;
        sample.clamp(min, max)
    }
}
