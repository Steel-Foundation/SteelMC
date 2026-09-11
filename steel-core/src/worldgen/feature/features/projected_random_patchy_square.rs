use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_projected_random_patchy_square_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &ProjectedRandomPatchySquareConfiguration,
        origin: BlockPos,
    ) -> bool {
        let size = config.size.sample(random);
        let bound = size * size + 1;

        for dx in -size..=size {
            for dz in -size..=size {
                let probability = dx.abs() * dz.abs();
                if random.next_i32_bounded(bound) >= bound - probability {
                    continue;
                }

                let mut base = origin.offset(dx, 0, dz);
                let mut drop = config.max_projection_height;
                while Self::test_block_predicate(
                    region,
                    registry,
                    &config.project_through,
                    base.below(),
                ) {
                    base = base.below();
                    drop -= 1;
                    if drop <= 0 {
                        break;
                    }
                }

                let Some(state) = Self::sample_block_state_provider_optional(
                    region,
                    registry,
                    random,
                    &config.block,
                    base,
                ) else {
                    continue;
                };
                let _ = region.set_block_state(base, state, UpdateFlags::UPDATE_CLIENTS);
            }
        }

        true
    }
}
