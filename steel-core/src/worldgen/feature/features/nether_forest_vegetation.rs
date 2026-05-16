use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_nether_forest_vegetation_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &NetherForestVegetationConfiguration,
        origin: BlockPos,
    ) -> bool {
        let below_state = region.block_state(origin.below());
        if !registry
            .blocks
            .is_in_tag(below_state.get_block(), &vanilla_block_tags::NYLIUM_TAG)
        {
            return false;
        }

        if origin.y() < region.min_y() + 1 || origin.y() + 1 > region.max_y_exclusive() - 1 {
            return false;
        }

        let mut placed = 0;
        for _ in 0..config.spread_width * config.spread_width {
            let final_pos = origin.offset(
                random.next_i32_bounded(config.spread_width)
                    - random.next_i32_bounded(config.spread_width),
                random.next_i32_bounded(config.spread_height)
                    - random.next_i32_bounded(config.spread_height),
                random.next_i32_bounded(config.spread_width)
                    - random.next_i32_bounded(config.spread_width),
            );
            let state = Self::sample_block_state_provider(
                region,
                registry,
                random,
                &config.state_provider,
                final_pos,
            );
            let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
            if region.block_state(final_pos).is_air()
                && final_pos.y() > region.min_y()
                && behavior.can_survive(state, region, final_pos)
            {
                let _ = region.set_block_state(final_pos, state, UpdateFlags::UPDATE_CLIENTS);
                placed += 1;
            }
        }

        placed > 0
    }
}
