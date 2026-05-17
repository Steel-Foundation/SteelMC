use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_huge_fungus_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &HugeFungusConfiguration,
        origin: BlockPos,
    ) -> bool {
        let valid_base_state = Self::block_state_from_data(registry, &config.valid_base_block);
        if region.block_state(origin.below()).get_block() != valid_base_state.get_block() {
            return false;
        }

        let mut total_height = random.next_i32_between(4, 13);
        if random.next_i32_bounded(12) == 0 {
            total_height *= 2;
        }

        if !config.planted && origin.y() + total_height + 1 >= region.generation_height() {
            return false;
        }

        let stem_state = Self::block_state_from_data(registry, &config.stem_state);
        let hat_state = Self::block_state_from_data(registry, &config.hat_state);
        let decor_state = Self::block_state_from_data(registry, &config.decor_state);
        let is_huge = !config.planted && random.next_f32() < 0.06;
        let _ = region.set_block_state(
            origin,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_NONE,
        );

        Self::place_huge_fungus_stem(
            region,
            registry,
            random,
            config,
            origin,
            stem_state,
            total_height,
            is_huge,
        );
        Self::place_huge_fungus_hat(
            region,
            registry,
            random,
            config,
            origin,
            hat_state,
            decor_state,
            total_height,
            is_huge,
        );
        true
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla HugeFungusFeature.placeStem state"
    )]
    fn place_huge_fungus_stem(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &HugeFungusConfiguration,
        origin: BlockPos,
        stem_state: BlockStateId,
        total_height: i32,
        is_huge: bool,
    ) {
        let stem_radius: i32 = i32::from(is_huge);

        for dx in -stem_radius..=stem_radius {
            for dz in -stem_radius..=stem_radius {
                let corner_of_huge_stem =
                    is_huge && dx.abs() == stem_radius && dz.abs() == stem_radius;

                for dy in 0..total_height {
                    let pos = origin.offset(dx, dy, dz);
                    if !Self::huge_fungus_is_replaceable(region, registry, pos, config, true) {
                        continue;
                    }

                    if config.planted {
                        if !region.block_state(pos.below()).is_air() {
                            let _ = region.set_block_state(
                                pos,
                                vanilla_blocks::AIR.default_state(),
                                UpdateFlags::UPDATE_NONE,
                            );
                        }

                        let _ = region.set_block_state(pos, stem_state, UpdateFlags::UPDATE_ALL);
                    } else if corner_of_huge_stem {
                        if random.next_f32() < 0.1 {
                            let _ =
                                region.set_block_state(pos, stem_state, UpdateFlags::UPDATE_ALL);
                        }
                    } else {
                        let _ = region.set_block_state(pos, stem_state, UpdateFlags::UPDATE_ALL);
                    }
                }
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla HugeFungusFeature.placeHat state"
    )]
    fn place_huge_fungus_hat(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &HugeFungusConfiguration,
        origin: BlockPos,
        hat_state: BlockStateId,
        decor_state: BlockStateId,
        total_height: i32,
        is_huge: bool,
    ) {
        let place_vines = hat_state.get_block() == &vanilla_blocks::NETHER_WART_BLOCK;
        let hat_height = (random.next_i32_bounded(1 + total_height / 3) + 5).min(total_height);
        let hat_start_y = total_height - hat_height;

        for dy in hat_start_y..=total_height {
            let mut radius = if dy < total_height - random.next_i32_bounded(3) {
                2
            } else {
                1
            };
            if hat_height > 8 && dy < hat_start_y + 4 {
                radius = 3;
            }

            if is_huge {
                radius += 1;
            }

            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let is_edge_x = dx == -radius || dx == radius;
                    let is_edge_z = dz == -radius || dz == radius;
                    let inside = !is_edge_x && !is_edge_z && dy != total_height;
                    let corner = is_edge_x && is_edge_z;
                    let is_hat_bottom = dy < hat_start_y + 3;
                    let pos = origin.offset(dx, dy, dz);

                    if !Self::huge_fungus_is_replaceable(region, registry, pos, config, false) {
                        continue;
                    }

                    if config.planted && !region.block_state(pos.below()).is_air() {
                        let _ = region.set_block_state(
                            pos,
                            vanilla_blocks::AIR.default_state(),
                            UpdateFlags::UPDATE_NONE,
                        );
                    }

                    if is_hat_bottom {
                        if !inside {
                            Self::place_huge_fungus_hat_drop_block(
                                region,
                                random,
                                pos,
                                hat_state,
                                place_vines,
                            );
                        }
                    } else if inside {
                        Self::place_huge_fungus_hat_block(
                            region,
                            random,
                            pos,
                            hat_state,
                            decor_state,
                            0.1,
                            0.2,
                            if place_vines { 0.1 } else { 0.0 },
                        );
                    } else if corner {
                        Self::place_huge_fungus_hat_block(
                            region,
                            random,
                            pos,
                            hat_state,
                            decor_state,
                            0.01,
                            0.7,
                            if place_vines { 0.083 } else { 0.0 },
                        );
                    } else {
                        Self::place_huge_fungus_hat_block(
                            region,
                            random,
                            pos,
                            hat_state,
                            decor_state,
                            5.0E-4,
                            0.98,
                            if place_vines { 0.07 } else { 0.0 },
                        );
                    }
                }
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "vanilla hat placement uses three independent probabilities"
    )]
    fn place_huge_fungus_hat_block(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        hat_state: BlockStateId,
        decor_state: BlockStateId,
        decor_block_probability: f32,
        hat_block_probability: f32,
        vines_probability: f32,
    ) {
        if random.next_f32() < decor_block_probability {
            let _ = region.set_block_state(pos, decor_state, UpdateFlags::UPDATE_ALL);
        } else if random.next_f32() < hat_block_probability {
            let _ = region.set_block_state(pos, hat_state, UpdateFlags::UPDATE_ALL);
            if random.next_f32() < vines_probability {
                Self::try_place_huge_fungus_weeping_vines(region, random, pos);
            }
        }
    }

    fn place_huge_fungus_hat_drop_block(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        hat_state: BlockStateId,
        place_vines: bool,
    ) {
        if region.block_state(pos.below()).get_block() == hat_state.get_block() {
            let _ = region.set_block_state(pos, hat_state, UpdateFlags::UPDATE_ALL);
        } else if random.next_f32() < 0.15 {
            let _ = region.set_block_state(pos, hat_state, UpdateFlags::UPDATE_ALL);
            if place_vines && random.next_i32_bounded(11) == 0 {
                Self::try_place_huge_fungus_weeping_vines(region, random, pos);
            }
        }
    }

    fn try_place_huge_fungus_weeping_vines(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        hat_pos: BlockPos,
    ) {
        let place_pos = hat_pos.below();
        if !region.block_state(place_pos).is_air() {
            return;
        }

        let mut goal_vine_height = random.next_i32_between(1, 5);
        if random.next_i32_bounded(7) == 0 {
            goal_vine_height *= 2;
        }

        Self::place_weeping_vines_column(region, random, place_pos, goal_vine_height, 23, 25);
    }

    fn huge_fungus_is_replaceable(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        pos: BlockPos,
        config: &HugeFungusConfiguration,
        check_non_replaceable_plants: bool,
    ) -> bool {
        if region.block_state(pos).is_replaceable() {
            return true;
        }

        check_non_replaceable_plants
            && Self::test_block_predicate(region, registry, &config.replaceable_blocks, pos)
    }
}
