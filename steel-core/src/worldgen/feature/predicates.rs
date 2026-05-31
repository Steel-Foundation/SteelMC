use super::prelude::*;
use super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(super) fn test_optional_block_predicate(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        predicate: Option<&BlockPredicate>,
        origin: BlockPos,
    ) -> bool {
        predicate
            .is_none_or(|predicate| Self::test_block_predicate(region, registry, predicate, origin))
    }

    pub(super) fn biome_allows_feature(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        biome_zoom_seed: i64,
        origin: BlockPos,
        biome_filter_feature_key: Option<&Identifier>,
    ) -> bool {
        let biome_id = fuzzed_biome_at_block(
            biome_zoom_seed,
            origin.x(),
            origin.y(),
            origin.z(),
            |quart_x, quart_y, quart_z| region.noise_biome_id(quart_x, quart_y, quart_z),
        );
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

    pub(super) fn test_block_predicate(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        predicate: &BlockPredicate,
        origin: BlockPos,
    ) -> bool {
        match predicate {
            BlockPredicate::AllOf { predicates } => predicates
                .iter()
                .all(|predicate| Self::test_block_predicate(region, registry, predicate, origin)),
            BlockPredicate::AnyOf { predicates } => predicates
                .iter()
                .any(|predicate| Self::test_block_predicate(region, registry, predicate, origin)),
            BlockPredicate::Not { predicate } => {
                !Self::test_block_predicate(region, registry, predicate, origin)
            }
            BlockPredicate::MatchingBlockTag { tag, offset } => {
                let state = region.block_state(Self::offset(origin, offset));
                state.get_block().has_tag(tag)
            }
            BlockPredicate::MatchingBlocks { blocks, offset } => {
                let state = region.block_state(Self::offset(origin, offset));
                blocks.0.contains(&state.get_block())
            }
            BlockPredicate::MatchingFluids { fluids, offset } => {
                let state = region.block_state(Self::offset(origin, offset));
                let fluid_state = get_fluid_state_from_block(state);
                fluids.0.contains(&fluid_state.fluid_id)
            }
            BlockPredicate::Solid { offset } => {
                region.block_state(Self::offset(origin, offset)).is_solid()
            }
            BlockPredicate::WouldSurvive { state, offset } => {
                let state = Self::block_state_from_data(registry, state);
                let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
                behavior.can_survive(state, region, Self::offset(origin, offset))
            }
            BlockPredicate::Replaceable { offset } => region
                .block_state(Self::offset(origin, offset))
                .is_replaceable(),
            BlockPredicate::HasSturdyFace { direction, offset } => region
                .block_state(Self::offset(origin, offset))
                .is_face_sturdy(*direction),
            BlockPredicate::InsideWorldBounds { offset } => {
                let position = Self::offset(origin, offset);
                !region.is_outside_build_height(position.y())
            }
        }
    }

    pub(super) const fn offset(origin: BlockPos, offset: &[i32; 3]) -> BlockPos {
        origin.offset(offset[0], offset[1], offset[2])
    }
}
