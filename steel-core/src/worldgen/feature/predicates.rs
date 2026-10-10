use super::prelude::*;
use super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(super) fn test_optional_block_predicate(
        level: &impl LevelReader,
        registry: &Registry,
        predicate: Option<&BlockPredicate>,
        origin: BlockPos,
    ) -> bool {
        predicate.is_none_or(|predicate| {
            BlockStateProviderEvaluator::test_block_predicate(level, registry, predicate, origin)
        })
    }

    pub(super) fn biome_allows_feature(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        biome_zoom_seed: i64,
        origin: BlockPos,
        biome_filter_feature_key: Option<&Identifier>,
    ) -> bool {
        let biome_id = fuzzed_biome_at_block(biome_zoom_seed, origin, |quart| {
            region.noise_biome_id(quart.x, quart.y, quart.z)
        });
        let Some(biome) = registry.biomes.by_id(usize::from(biome_id)) else {
            panic!("biome filter resolved unknown biome id {biome_id}");
        };
        let Some(target_feature_key) = biome_filter_feature_key else {
            panic!(
                "Tried to biome check an unregistered feature, or a feature that should not restrict the biome"
            );
        };

        biome
            .features
            .iter()
            .flatten()
            .any(|feature_key| feature_key == target_feature_key)
    }
}
