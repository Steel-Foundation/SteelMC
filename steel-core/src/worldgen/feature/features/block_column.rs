use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_block_column_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &BlockColumnConfiguration,
        origin: BlockPos,
    ) -> bool {
        let mut layer_heights = config
            .layers
            .iter()
            .map(|layer| layer.height.sample(random))
            .collect::<Vec<_>>();
        let total_height = layer_heights.iter().sum::<i32>();
        if total_height == 0 {
            return false;
        }

        let mut next_pos = origin.relative(config.direction);
        for height in 0..total_height {
            if !Self::test_block_predicate(region, registry, &config.allowed_placement, next_pos) {
                Self::truncate_block_column_layers(
                    &mut layer_heights,
                    total_height,
                    height,
                    config.prioritize_tip,
                );
                break;
            }
            next_pos = next_pos.relative(config.direction);
        }

        let mut place_pos = origin;
        for (layer_index, layer) in config.layers.iter().enumerate() {
            for _ in 0..layer_heights[layer_index] {
                let state = Self::sample_block_state_provider(
                    region,
                    registry,
                    random,
                    &layer.provider,
                    place_pos,
                );
                let _ = region.set_block_state(place_pos, state, UpdateFlags::UPDATE_CLIENTS);
                place_pos = place_pos.relative(config.direction);
            }
        }

        true
    }

    pub(in crate::worldgen::feature) fn truncate_block_column_layers(
        layer_heights: &mut [i32],
        total_height: i32,
        new_height: i32,
        prioritize_tip: bool,
    ) {
        let mut amount_to_remove = total_height - new_height;
        if prioritize_tip {
            for height in layer_heights {
                if amount_to_remove == 0 {
                    return;
                }
                let removed = (*height).min(amount_to_remove);
                amount_to_remove -= removed;
                *height -= removed;
            }
        } else {
            for height in layer_heights.iter_mut().rev() {
                if amount_to_remove == 0 {
                    return;
                }
                let removed = (*height).min(amount_to_remove);
                amount_to_remove -= removed;
                *height -= removed;
            }
        }
    }
}
