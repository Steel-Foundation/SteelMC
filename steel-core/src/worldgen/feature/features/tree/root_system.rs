use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_root_system_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &RootSystemConfiguration,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        if !region.block_state(origin).is_air() {
            return false;
        }

        let mut tree_pos = origin;
        if Self::place_root_system_dirt_and_tree(
            region,
            registry,
            random,
            config,
            &mut tree_pos,
            origin,
            biome_zoom_seed,
        ) {
            Self::place_root_system_hanging_roots(region, registry, random, config, origin);
        }

        true
    }

    fn place_root_system_dirt_and_tree(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &RootSystemConfiguration,
        tree_pos: &mut BlockPos,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        for y in 0..config.root_column_max_height {
            *tree_pos = tree_pos.above();
            if !Self::test_block_predicate(
                region,
                registry,
                &config.allowed_tree_position,
                *tree_pos,
            ) || !Self::root_system_space_for_tree(region, config, *tree_pos)
            {
                continue;
            }

            let below_pos = tree_pos.below();
            let below_state = region.block_state(below_pos);
            if get_fluid_state_from_block(below_state).is_lava() || !below_state.is_solid() {
                return false;
            }

            if Self::place_placed_feature_ref(
                region,
                registry,
                random,
                *tree_pos,
                &config.feature,
                biome_zoom_seed,
            ) {
                Self::place_root_system_dirt(
                    region,
                    registry,
                    random,
                    config,
                    origin,
                    origin.y() + y,
                );
                return true;
            }
        }

        false
    }

    fn root_system_space_for_tree(
        region: &WorldGenRegion<'_>,
        config: &RootSystemConfiguration,
        pos: BlockPos,
    ) -> bool {
        let mut column_pos = pos;
        for blocks_above_origin in 1..=config.required_vertical_space_for_tree {
            column_pos = column_pos.above();
            let state = region.block_state(column_pos);
            if !Self::root_system_allowed_tree_space(
                state,
                blocks_above_origin,
                config.allowed_vertical_water_for_tree,
            ) {
                return false;
            }
        }

        true
    }

    fn root_system_allowed_tree_space(
        state: BlockStateId,
        blocks_above_origin: i32,
        allowed_vertical_water_height: i32,
    ) -> bool {
        if state.is_air() {
            return true;
        }

        let blocks_above_ground = blocks_above_origin + 1;
        blocks_above_ground <= allowed_vertical_water_height
            && get_fluid_state_from_block(state).is_water()
    }

    fn place_root_system_dirt(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &RootSystemConfiguration,
        origin: BlockPos,
        target_height: i32,
    ) {
        for y in origin.y()..target_height {
            Self::place_root_system_rooted_dirt(
                region,
                registry,
                random,
                config,
                origin.at_y(y),
                origin.x(),
                origin.z(),
            );
        }
    }

    fn place_root_system_rooted_dirt(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &RootSystemConfiguration,
        mut pos: BlockPos,
        origin_x: i32,
        origin_z: i32,
    ) {
        for _ in 0..config.root_placement_attempts {
            pos = pos.offset(
                random.next_i32_bounded(config.root_radius)
                    - random.next_i32_bounded(config.root_radius),
                0,
                random.next_i32_bounded(config.root_radius)
                    - random.next_i32_bounded(config.root_radius),
            );

            let state = region.block_state(pos);
            if registry
                .blocks
                .is_in_tag(state.get_block(), &config.root_replaceable)
            {
                let replacement = Self::sample_block_state_provider(
                    region,
                    registry,
                    random,
                    &config.root_state_provider,
                    pos,
                );
                let _ = region.set_block_state(pos, replacement, UpdateFlags::UPDATE_CLIENTS);
            }

            pos = BlockPos::new(origin_x, pos.y(), origin_z);
        }
    }

    fn place_root_system_hanging_roots(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &RootSystemConfiguration,
        origin: BlockPos,
    ) {
        for _ in 0..config.hanging_root_placement_attempts {
            let pos = origin.offset(
                random.next_i32_bounded(config.hanging_root_radius)
                    - random.next_i32_bounded(config.hanging_root_radius),
                random.next_i32_bounded(config.hanging_roots_vertical_span)
                    - random.next_i32_bounded(config.hanging_roots_vertical_span),
                random.next_i32_bounded(config.hanging_root_radius)
                    - random.next_i32_bounded(config.hanging_root_radius),
            );

            if !region.block_state(pos).is_air() {
                continue;
            }

            let state = Self::sample_block_state_provider(
                region,
                registry,
                random,
                &config.hanging_root_state_provider,
                pos,
            );
            let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
            if behavior.can_survive(state, region, pos)
                && region
                    .block_state(pos.above())
                    .is_face_sturdy(Direction::Down)
            {
                let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
            }
        }
    }
}
