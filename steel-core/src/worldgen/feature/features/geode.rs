#![expect(
    clippy::too_many_lines,
    reason = "geode placement is kept linear to preserve vanilla parity"
)]

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;
use rustc_hash::FxHashMap;
use steel_utils::locks::SyncMutex;

static GEODE_NOISE_BY_SEED: LazyLock<SyncMutex<FxHashMap<i64, NormalNoise>>> =
    LazyLock::new(|| SyncMutex::new(FxHashMap::default()));

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_geode_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &GeodeConfiguration,
        origin: BlockPos,
    ) -> bool {
        let num_points = config.distribution_points.sample(random);
        let noise = Self::geode_noise(region.seed());
        let crack_size_adjustment =
            f64::from(num_points) / f64::from(config.outer_wall_distance.max());
        let layers = &config.layers;
        let blocks = &config.blocks;
        let crack = &config.crack;
        let inner_air = 1.0 / layers.filling.sqrt();
        let innermost_block_layer = 1.0 / (layers.inner_layer + crack_size_adjustment).sqrt();
        let inner_crust = 1.0 / (layers.middle_layer + crack_size_adjustment).sqrt();
        let outer_crust = 1.0 / (layers.outer_layer + crack_size_adjustment).sqrt();
        let crack_size = 1.0
            / (crack.base_crack_size
                + random.next_f64() / 2.0
                + if num_points > 3 {
                    crack_size_adjustment
                } else {
                    0.0
                })
            .sqrt();
        let should_generate_crack = random.next_f32() < crack.generate_crack_chance as f32;

        let Some(points) =
            Self::geode_distribution_points(region, random, registry, config, origin, num_points)
        else {
            return false;
        };
        let crack_points =
            Self::geode_crack_points(random, origin, num_points, should_generate_crack);

        let mut potential_crystal_placements = Vec::new();
        let min = origin.offset(
            config.min_gen_offset,
            config.min_gen_offset,
            config.min_gen_offset,
        );
        let max = origin.offset(
            config.max_gen_offset,
            config.max_gen_offset,
            config.max_gen_offset,
        );
        Self::for_each_vanilla_between_closed(min, max, |pos| {
            let noise_offset =
                noise.get_value(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()))
                    * config.noise_multiplier;
            let dist_sum_shell =
                Self::geode_distance_sum(pos, &points, noise_offset, |offset| offset);
            let dist_sum_crack = Self::geode_distance_sum(pos, &crack_points, noise_offset, |_| {
                crack.crack_point_offset
            });

            if dist_sum_shell >= outer_crust {
                if should_generate_crack
                    && dist_sum_crack >= crack_size
                    && dist_sum_shell < inner_air
                {
                    Self::safe_set_geode_block(
                        region,
                        registry,
                        pos,
                        vanilla_blocks::AIR.default_state(),
                        &blocks.cannot_replace,
                    );
                    Self::schedule_geode_adjacent_fluid_ticks(region, pos);
                } else if dist_sum_shell >= inner_air {
                    let state = Self::sample_block_state_provider(
                        region,
                        registry,
                        random,
                        &blocks.filling_provider,
                        pos,
                    );
                    Self::safe_set_geode_block(
                        region,
                        registry,
                        pos,
                        state,
                        &blocks.cannot_replace,
                    );
                } else if dist_sum_shell >= innermost_block_layer {
                    let use_alternate_layer =
                        random.next_f32() < config.use_alternate_layer0_chance as f32;
                    let provider = if use_alternate_layer {
                        &blocks.alternate_inner_layer_provider
                    } else {
                        &blocks.inner_layer_provider
                    };
                    let state =
                        Self::sample_block_state_provider(region, registry, random, provider, pos);
                    Self::safe_set_geode_block(
                        region,
                        registry,
                        pos,
                        state,
                        &blocks.cannot_replace,
                    );

                    if (!config.placements_require_layer0_alternate || use_alternate_layer)
                        && random.next_f32() < config.use_potential_placements_chance as f32
                    {
                        potential_crystal_placements.push(pos);
                    }
                } else if dist_sum_shell >= inner_crust {
                    let state = Self::sample_block_state_provider(
                        region,
                        registry,
                        random,
                        &blocks.middle_layer_provider,
                        pos,
                    );
                    Self::safe_set_geode_block(
                        region,
                        registry,
                        pos,
                        state,
                        &blocks.cannot_replace,
                    );
                } else if dist_sum_shell >= outer_crust {
                    let state = Self::sample_block_state_provider(
                        region,
                        registry,
                        random,
                        &blocks.outer_layer_provider,
                        pos,
                    );
                    Self::safe_set_geode_block(
                        region,
                        registry,
                        pos,
                        state,
                        &blocks.cannot_replace,
                    );
                }
            }
        });

        Self::place_geode_crystals(
            region,
            registry,
            random,
            blocks,
            &potential_crystal_placements,
        );
        true
    }

    fn geode_noise(seed: i64) -> NormalNoise {
        let mut cache = GEODE_NOISE_BY_SEED.lock();
        if let Some(noise) = cache.get(&seed) {
            return noise.clone();
        }

        let mut random_source = RandomSource::Legacy(LegacyRandom::from_seed(seed as u64));
        let noise = NormalNoise::create_from_random(&mut random_source, -4, &[1.0]);
        cache.insert(seed, noise.clone());
        noise
    }

    fn geode_distribution_points(
        region: &WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        registry: &Registry,
        config: &GeodeConfiguration,
        origin: BlockPos,
        num_points: i32,
    ) -> Option<Vec<(BlockPos, i32)>> {
        let mut points = Vec::new();
        let mut invalid_points = 0;
        for _ in 0..num_points {
            let point = origin.offset(
                config.outer_wall_distance.sample(random),
                config.outer_wall_distance.sample(random),
                config.outer_wall_distance.sample(random),
            );
            let state = region.block_state(point);
            if state.is_air()
                || registry
                    .blocks
                    .is_in_tag(state.get_block(), &config.blocks.invalid_blocks)
            {
                invalid_points += 1;
                if invalid_points > config.invalid_blocks_threshold {
                    return None;
                }
            }

            points.push((point, config.point_offset.sample(random)));
        }

        Some(points)
    }

    fn geode_crack_points(
        random: &mut WorldgenRandom,
        origin: BlockPos,
        num_points: i32,
        should_generate_crack: bool,
    ) -> Vec<(BlockPos, i32)> {
        if !should_generate_crack {
            return Vec::new();
        }

        let crack_offset = num_points * 2 + 1;
        match random.next_i32_bounded(4) {
            0 => vec![
                (origin.offset(crack_offset, 7, 0), 0),
                (origin.offset(crack_offset, 5, 0), 0),
                (origin.offset(crack_offset, 1, 0), 0),
            ],
            1 => vec![
                (origin.offset(0, 7, crack_offset), 0),
                (origin.offset(0, 5, crack_offset), 0),
                (origin.offset(0, 1, crack_offset), 0),
            ],
            2 => vec![
                (origin.offset(crack_offset, 7, crack_offset), 0),
                (origin.offset(crack_offset, 5, crack_offset), 0),
                (origin.offset(crack_offset, 1, crack_offset), 0),
            ],
            _ => vec![
                (origin.offset(0, 7, 0), 0),
                (origin.offset(0, 5, 0), 0),
                (origin.offset(0, 1, 0), 0),
            ],
        }
    }

    fn geode_distance_sum(
        pos: BlockPos,
        points: &[(BlockPos, i32)],
        noise_offset: f64,
        offset: impl Fn(i32) -> i32,
    ) -> f64 {
        points
            .iter()
            .map(|(point, point_offset)| {
                1.0 / (Self::block_pos_distance_squared(pos, *point)
                    + f64::from(offset(*point_offset)))
                .sqrt()
                    + noise_offset
            })
            .sum()
    }

    fn block_pos_distance_squared(left: BlockPos, right: BlockPos) -> f64 {
        let dx = f64::from(left.x() - right.x());
        let dy = f64::from(left.y() - right.y());
        let dz = f64::from(left.z() - right.z());
        dx * dx + dy * dy + dz * dz
    }

    fn safe_set_geode_block(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        pos: BlockPos,
        state: BlockStateId,
        cannot_replace: &Identifier,
    ) {
        let current = region.block_state(pos);
        if !registry
            .blocks
            .is_in_tag(current.get_block(), cannot_replace)
        {
            let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
        }
    }

    fn schedule_geode_adjacent_fluid_ticks(region: &WorldGenRegion<'_>, pos: BlockPos) {
        for direction in Self::VANILLA_DIRECTION_VALUES {
            let adjacent = pos.relative(direction);
            let fluid_state = get_fluid_state_from_block(region.block_state(adjacent));
            if !fluid_state.is_empty() {
                let _ = region.schedule_fluid_tick_default(adjacent, fluid_state.fluid_id, 0);
            }
        }
    }

    fn place_geode_crystals(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        blocks: &GeodeBlockSettings,
        potential_crystal_placements: &[BlockPos],
    ) {
        for crystal_pos in potential_crystal_placements {
            if blocks.inner_placements.is_empty() {
                continue;
            }
            let Ok(bound) = i32::try_from(blocks.inner_placements.len()) else {
                panic!(
                    "geode inner placement count {} exceeds i32 range",
                    blocks.inner_placements.len()
                );
            };
            let index = random.next_i32_bounded(bound) as usize;
            let mut state = Self::block_state_from_data(registry, &blocks.inner_placements[index]);

            for direction in Self::VANILLA_DIRECTION_VALUES {
                if state.try_get_value(&BlockStateProperties::FACING).is_some() {
                    state = state.set_value(&BlockStateProperties::FACING, direction);
                }

                let place_pos = crystal_pos.relative(direction);
                let place_state = region.block_state(place_pos);
                if state
                    .try_get_value(&BlockStateProperties::WATERLOGGED)
                    .is_some()
                {
                    let waterlogged = place_state.get_block() == &vanilla_blocks::WATER
                        && get_fluid_state_from_block(place_state).is_source();
                    state = state.set_value(&BlockStateProperties::WATERLOGGED, waterlogged);
                }

                if Self::can_grow_amethyst_cluster_at(place_state) {
                    Self::safe_set_geode_block(
                        region,
                        registry,
                        place_pos,
                        state,
                        &blocks.cannot_replace,
                    );
                    break;
                }
            }
        }
    }

    fn can_grow_amethyst_cluster_at(state: BlockStateId) -> bool {
        state.is_air()
            || (state.get_block() == &vanilla_blocks::WATER
                && get_fluid_state_from_block(state).is_source())
    }
}
