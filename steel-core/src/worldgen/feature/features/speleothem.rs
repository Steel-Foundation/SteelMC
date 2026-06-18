use steel_registry::vanilla_block_tags::BlockTag;

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_speleothem_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        config: &SpeleothemConfiguration,
        origin: BlockPos,
    ) -> bool {
        let Some(tip_direction) = Self::speleothem_tip_direction(region, random, config, origin)
        else {
            return false;
        };

        let root_pos = origin.relative(tip_direction.opposite());
        Self::create_patch_of_speleothem_base_blocks(region, random, root_pos, config);
        let height = if random.next_f32() < config.chance_of_taller_generation
            && Self::speleothem_is_empty_or_water(
                region.block_state(origin.relative(tip_direction)),
            ) {
            2
        } else {
            1
        };
        Self::grow_speleothem(
            region,
            origin,
            tip_direction,
            height,
            false,
            config.base_block.block,
            config.pointed_block.block,
            &config.replaceable_blocks,
        );
        true
    }

    pub(in crate::worldgen::feature) fn place_speleothem_cluster_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        config: &SpeleothemClusterConfiguration,
        origin: BlockPos,
    ) -> bool {
        if !Self::speleothem_is_empty_or_water(region.block_state(origin)) {
            return false;
        }

        let height = config.height.sample(random);
        let wetness = config.wetness.sample(random);
        let density = config.density.sample(random);
        let x_radius = config.radius.sample(random);
        let z_radius = config.radius.sample(random);

        for dx in -x_radius..=x_radius {
            for dz in -z_radius..=z_radius {
                let chance = Self::chance_of_speleothem(x_radius, z_radius, dx, dz, config);
                let pos = origin.offset(dx, 0, dz);
                Self::place_speleothem_cluster_column(
                    region, random, pos, dx, dz, wetness, chance, height, density, config,
                );
            }
        }

        true
    }

    fn speleothem_tip_direction(
        region: &WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        config: &SpeleothemConfiguration,
        pos: BlockPos,
    ) -> Option<Direction> {
        let can_place_above = Self::is_speleothem_base(
            region.block_state(pos.above()),
            config.base_block.block,
            &config.replaceable_blocks,
        );
        let can_place_below = Self::is_speleothem_base(
            region.block_state(pos.below()),
            config.base_block.block,
            &config.replaceable_blocks,
        );
        if can_place_above && can_place_below {
            Some(if random.next_bool() {
                Direction::Down
            } else {
                Direction::Up
            })
        } else if can_place_above {
            Some(Direction::Down)
        } else if can_place_below {
            Some(Direction::Up)
        } else {
            None
        }
    }

    fn create_patch_of_speleothem_base_blocks(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        config: &SpeleothemConfiguration,
    ) {
        Self::place_speleothem_base_block_if_possible(
            region,
            pos,
            config.base_block.block,
            &config.replaceable_blocks,
        );

        for direction in Self::VANILLA_HORIZONTAL_DIRECTIONS {
            if random.next_f32() > config.chance_of_directional_spread {
                continue;
            }

            let pos1 = pos.relative(direction);
            Self::place_speleothem_base_block_if_possible(
                region,
                pos1,
                config.base_block.block,
                &config.replaceable_blocks,
            );
            if random.next_f32() > config.chance_of_spread_radius2 {
                continue;
            }

            let pos2 = pos1.relative(Self::random_speleothem_direction(random));
            Self::place_speleothem_base_block_if_possible(
                region,
                pos2,
                config.base_block.block,
                &config.replaceable_blocks,
            );
            if random.next_f32() > config.chance_of_spread_radius3 {
                continue;
            }

            let pos3 = pos2.relative(Self::random_speleothem_direction(random));
            Self::place_speleothem_base_block_if_possible(
                region,
                pos3,
                config.base_block.block,
                &config.replaceable_blocks,
            );
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla SpeleothemClusterFeature.placeColumn state"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "mirrors vanilla SpeleothemClusterFeature.placeColumn flow"
    )]
    fn place_speleothem_cluster_column(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        dx: i32,
        dz: i32,
        chance_of_water: f32,
        chance_of_stalagmite_or_stalactite: f64,
        cluster_height: i32,
        density: f32,
        config: &SpeleothemClusterConfiguration,
    ) {
        let Some(base_column) = Self::scan_dripstone_column(
            region,
            pos,
            config.floor_to_ceiling_search_range,
            Self::speleothem_is_empty_or_water,
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
            if want_pool && Self::can_place_speleothem_pool(region, pos.at_y(base_floor_y), config)
            {
                let _ = region.set_block_state(
                    pos.at_y(base_floor_y),
                    vanilla_blocks::WATER.default_state(),
                    UpdateFlags::UPDATE_CLIENTS,
                );
                let mut column = base_column;
                column.floor = Some(base_floor_y - 1);
                column
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
                let ceiling_thickness = config.speleothem_block_layer_thickness.sample(random);
                Self::replace_blocks_with_speleothem_base_blocks(
                    region,
                    pos.at_y(ceiling_y),
                    ceiling_thickness,
                    Direction::Up,
                    config,
                );
                let max_height = if let Some(floor_y) = floor {
                    cluster_height.min(ceiling_y - floor_y)
                } else {
                    cluster_height
                };
                Self::speleothem_cluster_height(random, dx, dz, density, max_height, config)
            } else {
                0
            }
        } else {
            0
        };

        let want_stalagmite = random.next_f64() < chance_of_stalagmite_or_stalactite;
        let stalagmite_height = if let Some(floor_y) = floor {
            if want_stalagmite && !Self::is_lava(region, pos.at_y(floor_y)) {
                let floor_thickness = config.speleothem_block_layer_thickness.sample(random);
                Self::replace_blocks_with_speleothem_base_blocks(
                    region,
                    pos.at_y(floor_y),
                    floor_thickness,
                    Direction::Down,
                    config,
                );
                if ceiling.is_some() {
                    (stalactite_height
                        + random.next_i32_between(
                            -config.max_stalagmite_stalactite_height_diff,
                            config.max_stalagmite_stalactite_height_diff,
                        ))
                    .max(0)
                } else {
                    Self::speleothem_cluster_height(random, dx, dz, density, cluster_height, config)
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
            && speleothem_column_height(column.floor, column.ceiling).is_some_and(|height| {
                actual_stalactite_height + actual_stalagmite_height == height
            });

        if let Some(ceiling_y) = ceiling {
            Self::grow_speleothem(
                region,
                pos.at_y(ceiling_y - 1),
                Direction::Down,
                actual_stalactite_height,
                merge_tips,
                config.base_block.block,
                config.pointed_block.block,
                &config.replaceable_blocks,
            );
        }

        if let Some(floor_y) = floor {
            Self::grow_speleothem(
                region,
                pos.at_y(floor_y + 1),
                Direction::Up,
                actual_stalagmite_height,
                merge_tips,
                config.base_block.block,
                config.pointed_block.block,
                &config.replaceable_blocks,
            );
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "keeps vanilla speleothem growth parameters explicit"
    )]
    fn grow_speleothem(
        region: &mut WorldGenRegion<'_>,
        start_pos: BlockPos,
        tip_direction: Direction,
        height: i32,
        merged_tip: bool,
        base_block: BlockRef,
        pointed_block: BlockRef,
        replaceable_blocks: &BlockHolderSet,
    ) {
        if !Self::is_speleothem_base(
            region.block_state(start_pos.relative(tip_direction.opposite())),
            base_block,
            replaceable_blocks,
        ) {
            return;
        }

        let mut pos = start_pos;
        Self::build_base_to_tip_speleothem_column(
            tip_direction,
            height,
            merged_tip,
            pointed_block,
            |state| {
                let state = if state.get_block() == pointed_block {
                    state.set_value(
                        &BlockStateProperties::WATERLOGGED,
                        get_fluid_state_from_block(region.block_state(pos)).is_water(),
                    )
                } else {
                    state
                };

                let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
                pos = pos.relative(tip_direction);
            },
        );
    }

    fn build_base_to_tip_speleothem_column(
        direction: Direction,
        total_length: i32,
        merged_tip: bool,
        pointed_block: BlockRef,
        mut consumer: impl FnMut(BlockStateId),
    ) {
        if total_length >= 3 {
            consumer(Self::create_pointed_speleothem(
                pointed_block,
                direction,
                SpeleothemThickness::Base,
            ));

            for _ in 0..total_length - 3 {
                consumer(Self::create_pointed_speleothem(
                    pointed_block,
                    direction,
                    SpeleothemThickness::Middle,
                ));
            }
        }

        if total_length >= 2 {
            consumer(Self::create_pointed_speleothem(
                pointed_block,
                direction,
                SpeleothemThickness::Frustum,
            ));
        }

        if total_length >= 1 {
            let thickness = if merged_tip {
                SpeleothemThickness::TipMerge
            } else {
                SpeleothemThickness::Tip
            };
            consumer(Self::create_pointed_speleothem(
                pointed_block,
                direction,
                thickness,
            ));
        }
    }

    fn create_pointed_speleothem(
        pointed_block: BlockRef,
        direction: Direction,
        thickness: SpeleothemThickness,
    ) -> BlockStateId {
        pointed_block
            .default_state()
            .set_value(&BlockStateProperties::VERTICAL_DIRECTION, direction)
            .set_value(&BlockStateProperties::SPELEOTHEM_THICKNESS, thickness)
    }

    fn place_speleothem_base_block_if_possible(
        region: &mut WorldGenRegion<'_>,
        pos: BlockPos,
        base_block: BlockRef,
        replaceable_blocks: &BlockHolderSet,
    ) -> bool {
        if !Self::block_matches_holder_set(region.block_state(pos).get_block(), replaceable_blocks)
        {
            return false;
        }

        region.set_block_state(pos, base_block.default_state(), UpdateFlags::UPDATE_CLIENTS)
    }

    fn is_speleothem_base(
        state: BlockStateId,
        base_block: BlockRef,
        replaceable_blocks: &BlockHolderSet,
    ) -> bool {
        let block = state.get_block();
        block == base_block || Self::block_matches_holder_set(block, replaceable_blocks)
    }

    fn can_place_speleothem_pool(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        config: &SpeleothemClusterConfiguration,
    ) -> bool {
        let state = region.block_state(pos);
        let block = state.get_block();
        if block == &vanilla_blocks::WATER
            || block == config.base_block.block
            || block == config.pointed_block.block
        {
            return false;
        }

        if get_fluid_state_from_block(region.block_state(pos.above())).is_water() {
            return false;
        }

        for direction in Self::VANILLA_HORIZONTAL_DIRECTIONS {
            if !Self::can_be_adjacent_to_speleothem_pool_water(region, pos.relative(direction)) {
                return false;
            }
        }

        Self::can_be_adjacent_to_speleothem_pool_water(region, pos.below())
    }

    fn can_be_adjacent_to_speleothem_pool_water(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
    ) -> bool {
        let state = region.block_state(pos);
        state.get_block().has_tag(&BlockTag::BASE_STONE_OVERWORLD)
            || get_fluid_state_from_block(state).is_water()
    }

    fn replace_blocks_with_speleothem_base_blocks(
        region: &mut WorldGenRegion<'_>,
        mut pos: BlockPos,
        max_count: i32,
        direction: Direction,
        config: &SpeleothemClusterConfiguration,
    ) {
        for _ in 0..max_count {
            if !Self::place_speleothem_base_block_if_possible(
                region,
                pos,
                config.base_block.block,
                &config.replaceable_blocks,
            ) {
                return;
            }

            pos = pos.relative(direction);
        }
    }

    fn speleothem_cluster_height(
        random: &mut WorldgenRandom,
        dx: i32,
        dz: i32,
        density: f32,
        max_height: i32,
        config: &SpeleothemClusterConfiguration,
    ) -> i32 {
        if random.next_f32() > density {
            return 0;
        }

        let distance_from_center = dx.abs() + dz.abs();
        let height_mean = Self::clamped_map_f32(
            distance_from_center as f32,
            0.0,
            config.max_distance_from_center_affecting_height_bias as f32,
            max_height as f32 / 2.0,
            0.0,
        );
        Self::speleothem_random_between_biased(
            random,
            0.0,
            max_height as f32,
            height_mean,
            config.height_deviation as f32,
        ) as i32
    }

    fn chance_of_speleothem(
        x_radius: i32,
        z_radius: i32,
        dx: i32,
        dz: i32,
        config: &SpeleothemClusterConfiguration,
    ) -> f64 {
        let x_distance_from_edge = x_radius - dx.abs();
        let z_distance_from_edge = z_radius - dz.abs();
        let distance_from_edge = x_distance_from_edge.min(z_distance_from_edge);
        f64::from(Self::clamped_map_f32(
            distance_from_edge as f32,
            0.0,
            config.max_distance_from_edge_affecting_chance_of_speleothem as f32,
            config.chance_of_speleothem_at_max_distance_from_center,
            1.0,
        ))
    }

    fn speleothem_random_between_biased(
        random: &mut WorldgenRandom,
        min: f32,
        max: f32,
        mean: f32,
        deviation: f32,
    ) -> f32 {
        let sample = mean + deviation * random.next_gaussian() as f32;
        sample.clamp(min, max)
    }

    fn speleothem_is_empty_or_water(state: BlockStateId) -> bool {
        state.is_air() || state.get_block() == &vanilla_blocks::WATER
    }

    fn random_speleothem_direction(random: &mut WorldgenRandom) -> Direction {
        Self::VANILLA_DIRECTION_VALUES[random.next_i32_bounded(6) as usize]
    }
}

const fn speleothem_column_height(floor: Option<i32>, ceiling: Option<i32>) -> Option<i32> {
    match (floor, ceiling) {
        (Some(floor), Some(ceiling)) => Some(ceiling - floor - 1),
        _ => None,
    }
}
