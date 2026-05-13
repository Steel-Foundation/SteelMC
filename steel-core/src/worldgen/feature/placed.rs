use super::prelude::*;
use super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(super) fn place_placed_feature_entry(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        origin: BlockPos,
        feature: PlacedFeatureEntryRef,
        biome_zoom_seed: i64,
    ) -> bool {
        let Some(feature_id) = feature.try_id() else {
            panic!("top-level placed feature {} is not registered", feature.key);
        };
        Self::place_placed_feature_data(
            region,
            registry,
            random,
            origin,
            &feature.data,
            Some(feature_id),
            biome_zoom_seed,
        )
    }

    pub(super) fn place_placed_feature_data(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        origin: BlockPos,
        feature: &PlacedFeatureData,
        biome_filter_feature_id: Option<usize>,
        biome_zoom_seed: i64,
    ) -> bool {
        let mut positions = vec![origin];

        for modifier in &feature.placement {
            let mut next_positions = Vec::new();
            for position in positions {
                next_positions.extend(Self::apply_placement_modifier(
                    region,
                    registry,
                    random,
                    position,
                    biome_filter_feature_id,
                    modifier,
                    biome_zoom_seed,
                ));
            }

            if next_positions.is_empty() {
                return false;
            }
            positions = next_positions;
        }

        let mut placed = false;
        for position in positions {
            placed |= Self::place_configured_feature(
                region,
                registry,
                random,
                &feature.feature,
                position,
                biome_zoom_seed,
            );
        }
        placed
    }

    pub(super) fn place_placed_feature_ref(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        origin: BlockPos,
        feature: &PlacedFeatureRef,
        biome_zoom_seed: i64,
    ) -> bool {
        let feature_data = match feature {
            PlacedFeatureRef::Reference(key) => {
                let Some(feature) = registry.placed_features.by_key(key) else {
                    panic!("configured selector references unknown placed feature {key}");
                };
                &feature.data
            }
            PlacedFeatureRef::Inline(feature) => feature,
        };

        Self::place_placed_feature_data(
            region,
            registry,
            random,
            origin,
            feature_data,
            None,
            biome_zoom_seed,
        )
    }
}
