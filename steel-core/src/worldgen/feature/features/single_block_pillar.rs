use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_single_block_pillar_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &SingleBlockPillarConfiguration,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        let mut pos = origin;
        while Self::test_block_predicate(region, registry, &config.can_replace, pos)
            && random.next_f32() < config.chance_to_continue
            && !region.is_outside_build_height(pos.y())
        {
            let state =
                Self::sample_block_state_provider(region, registry, random, &config.block, pos);
            let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
            pos = pos.relative(config.direction);
        }

        pos = pos.relative(config.direction.opposite());
        if let Some(cap_feature) = &config.cap_feature {
            Self::place_placed_feature_ref(
                region,
                registry,
                random,
                pos,
                cap_feature,
                biome_zoom_seed,
            );
        }

        true
    }
}
