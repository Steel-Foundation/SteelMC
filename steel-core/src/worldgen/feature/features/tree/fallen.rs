use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;
use super::TreePlacement;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_fallen_tree_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &FallenTreeConfiguration,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        Self::place_fallen_tree_stump(region, registry, random, config, origin, biome_zoom_seed);

        let direction = Self::random_horizontal_direction(random);
        let log_length = config.log_length.sample(random) - 2;
        let mut log_start_pos = origin.relative_n(direction, 2 + random.next_i32_bounded(2));
        Self::set_ground_height_for_fallen_log_start_pos(region, &mut log_start_pos);
        if Self::can_place_entire_fallen_log(region, log_length, &mut log_start_pos, direction) {
            Self::place_fallen_log(
                region,
                registry,
                random,
                config,
                log_length,
                log_start_pos,
                direction,
                biome_zoom_seed,
            );
        }

        true
    }

    fn set_ground_height_for_fallen_log_start_pos(
        region: &WorldGenRegion<'_>,
        log_start_pos: &mut BlockPos,
    ) {
        *log_start_pos = log_start_pos.above();

        for _ in 0..6 {
            if Self::may_place_fallen_log_on(region, *log_start_pos) {
                return;
            }

            *log_start_pos = log_start_pos.below();
        }
    }

    fn place_fallen_tree_stump(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &FallenTreeConfiguration,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) {
        let mut placement = TreePlacement::default();
        Self::place_fallen_log_block(
            region,
            registry,
            random,
            config,
            origin,
            None,
            &mut placement,
        );
        Self::place_tree_decorators(
            region,
            registry,
            random,
            &config.stump_decorators,
            &mut placement,
            biome_zoom_seed,
        );
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla FallenTreeFeature.placeFallenLog state"
    )]
    fn place_fallen_log(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &FallenTreeConfiguration,
        log_length: i32,
        mut log_start_pos: BlockPos,
        direction: Direction,
        biome_zoom_seed: i64,
    ) {
        let mut placement = TreePlacement::default();

        for _ in 0..log_length {
            Self::place_fallen_log_block(
                region,
                registry,
                random,
                config,
                log_start_pos,
                Some(direction.axis()),
                &mut placement,
            );
            log_start_pos = log_start_pos.relative(direction);
        }

        Self::place_tree_decorators(
            region,
            registry,
            random,
            &config.log_decorators,
            &mut placement,
            biome_zoom_seed,
        );
    }

    fn can_place_entire_fallen_log(
        region: &WorldGenRegion<'_>,
        log_length: i32,
        log_start_pos: &mut BlockPos,
        direction: Direction,
    ) -> bool {
        let mut gap_in_ground = 0;

        for _ in 0..log_length {
            if !Self::tree_valid_pos(region, *log_start_pos) {
                return false;
            }

            if Self::is_over_solid_ground_for_fallen_log(region, *log_start_pos) {
                gap_in_ground = 0;
            } else {
                gap_in_ground += 1;
                if gap_in_ground > 2 {
                    return false;
                }
            }

            *log_start_pos = log_start_pos.relative(direction);
        }

        *log_start_pos = log_start_pos.relative_n(direction.opposite(), log_length);
        true
    }

    fn may_place_fallen_log_on(region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        Self::tree_valid_pos(region, pos) && Self::is_over_solid_ground_for_fallen_log(region, pos)
    }

    fn is_over_solid_ground_for_fallen_log(region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        region
            .block_state(below_pos)
            .is_face_sturdy_at(below_pos, Direction::Up)
    }

    fn place_fallen_log_block(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &FallenTreeConfiguration,
        pos: BlockPos,
        axis: Option<Axis>,
        placement: &mut TreePlacement,
    ) {
        let mut state = Self::sample_block_state_provider(
            region,
            registry,
            random,
            &config.trunk_provider,
            pos,
        );
        if let Some(axis) = axis
            && state.try_get_value(&BlockStateProperties::AXIS).is_some()
        {
            state = state.set_value(&BlockStateProperties::AXIS, axis);
        }

        placement.set_trunk(region, pos, state);
        Self::mark_above_for_postprocessing(region, pos);
    }
}
