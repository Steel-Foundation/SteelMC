use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_random_neighbor_spread_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &RandomNeighborSpreadConfiguration,
        origin: BlockPos,
    ) -> bool {
        let origin_state =
            Self::sample_block_state_provider(region, registry, random, &config.block, origin);
        let _ = region.set_block_state(origin, origin_state, UpdateFlags::UPDATE_CLIENTS);

        let attempts = config.attempts.sample(random);
        for _ in 0..attempts {
            let place_pos = origin.offset(
                config.xz_offset.sample(random),
                config.y_offset.sample(random),
                config.xz_offset.sample(random),
            );
            if !Self::test_block_predicate(region, registry, &config.can_replace, place_pos) {
                continue;
            }

            let mut neighbors = 0;
            for direction in Self::VANILLA_DIRECTION_VALUES {
                let block = region
                    .block_state(place_pos.relative(direction))
                    .get_block();
                if Self::block_matches_holder_set(block, &config.accepted_neighbors) {
                    neighbors += 1;
                }
                if neighbors > 1 {
                    break;
                }
            }

            if neighbors == 1 {
                let state = Self::sample_block_state_provider(
                    region,
                    registry,
                    random,
                    &config.block,
                    place_pos,
                );
                let _ = region.set_block_state(place_pos, state, UpdateFlags::UPDATE_CLIENTS);
            }
        }

        true
    }
}
