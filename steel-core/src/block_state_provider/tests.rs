use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::feature::{
    BlockHolderSet, BlockPredicate, BlockStateData, BlockStateProviderKind,
    RuleBasedStateProviderRule, WeightedBlockState,
};
use steel_registry::{REGISTRY, init_vanilla_registry, vanilla_blocks};
use steel_utils::random::{
    Random as _, legacy_random::LegacyRandom, worldgen_random::WorldgenRandom,
};
use steel_utils::{BlockPos, Direction, Identifier};

use crate::test_support::TestLevel;

use super::BlockStateProviderEvaluator;

#[test]
fn empty_random_block_providers_preserve_the_block_without_consuming_randomness() {
    init_vanilla_registry();
    let state = vanilla_blocks::DIRT.default_state();
    let level = TestLevel::default().with_block(BlockPos::ZERO, state);
    let mut random = WorldgenRandom::from_seed(42);
    let mut untouched = WorldgenRandom::from_seed(42);
    for blocks in [
        BlockHolderSet::Entries(Vec::new()),
        BlockHolderSet::Tag(Identifier::vanilla_static("test/empty_provider")),
    ] {
        let provider = BlockStateProviderKind::RandomBlock { blocks };
        assert!(
            BlockStateProviderEvaluator::sample_block_state_provider_optional(
                &level,
                &REGISTRY,
                &mut random,
                &provider,
                BlockPos::ZERO,
            )
            .is_none()
        );
        assert_eq!(
            BlockStateProviderEvaluator::sample_block_state_provider(
                &level,
                &REGISTRY,
                &mut random,
                &provider,
                BlockPos::ZERO,
            ),
            state
        );
    }
    assert_eq!(random.next_i64(), untouched.next_i64());
}

#[test]
fn matching_nested_rule_without_a_result_continues_to_the_next_rule() {
    init_vanilla_registry();
    let level =
        TestLevel::default().with_block(BlockPos::ZERO, vanilla_blocks::DIRT.default_state());
    let empty = BlockStateProviderKind::RuleBased {
        fallback: None,
        rules: Vec::new(),
    };
    let provider = BlockStateProviderKind::RuleBased {
        fallback: None,
        rules: vec![
            RuleBasedStateProviderRule {
                if_true: BlockPredicate::True,
                then: empty,
            },
            RuleBasedStateProviderRule {
                if_true: BlockPredicate::True,
                then: BlockStateProviderKind::Simple {
                    state: BlockStateData {
                        block: &vanilla_blocks::STONE,
                        properties: &[],
                    },
                },
            },
        ],
    };
    let mut random = LegacyRandom::from_seed(42);
    assert_eq!(
        BlockStateProviderEvaluator::sample_block_state_provider_optional(
            &level,
            &REGISTRY,
            &mut random,
            &provider,
            BlockPos::ZERO,
        ),
        Some(vanilla_blocks::STONE.default_state()),
    );

    let empty = BlockStateProviderKind::RuleBased {
        fallback: None,
        rules: Vec::new(),
    };
    assert!(
        BlockStateProviderEvaluator::sample_block_state_provider_optional(
            &level,
            &REGISTRY,
            &mut random,
            &empty,
            BlockPos::ZERO,
        )
        .is_none()
    );
    assert_eq!(
        BlockStateProviderEvaluator::sample_block_state_provider(
            &level,
            &REGISTRY,
            &mut random,
            &empty,
            BlockPos::ZERO,
        ),
        vanilla_blocks::DIRT.default_state()
    );
}

#[test]
fn rotated_provider_preserves_worldgen_rng_order_before_sampling_its_source() {
    init_vanilla_registry();
    let provider = BlockStateProviderKind::RotatedBlock {
        direction: None,
        state: Box::new(BlockStateProviderKind::Weighted {
            entries: vec![
                WeightedBlockState {
                    data: BlockStateData {
                        block: &vanilla_blocks::STONE,
                        properties: &[],
                    },
                    weight: 0,
                },
                WeightedBlockState {
                    data: BlockStateData {
                        block: &vanilla_blocks::OAK_LOG,
                        properties: &[],
                    },
                    weight: 1,
                },
                WeightedBlockState {
                    data: BlockStateData {
                        block: &vanilla_blocks::BIRCH_LOG,
                        properties: &[],
                    },
                    weight: 2,
                },
            ],
        }),
    };
    for seed in 0..64 {
        let mut random = WorldgenRandom::from_seed(seed);
        let mut expected_random = WorldgenRandom::from_seed(seed);
        let direction = Direction::ALL[expected_random.next_i32_bounded(6) as usize];
        let block = if expected_random.next_i32_bounded(3) == 0 {
            &vanilla_blocks::OAK_LOG
        } else {
            &vanilla_blocks::BIRCH_LOG
        };
        let state = BlockStateProviderEvaluator::sample_block_state_provider(
            &TestLevel::default(),
            &REGISTRY,
            &mut random,
            &provider,
            BlockPos::ZERO,
        );
        assert_eq!(
            state,
            block
                .default_state()
                .set_value(&BlockStateProperties::AXIS, direction.axis())
        );
        assert_eq!(random.next_i64(), expected_random.next_i64());
    }
}

#[test]
fn shipped_transformer_providers_never_consume_randomness() {
    init_vanilla_registry();
    let mut random = WorldgenRandom::from_seed(54);
    let mut untouched = WorldgenRandom::from_seed(54);
    for (_, block) in REGISTRY.blocks.iter() {
        let level = TestLevel::default().with_block(BlockPos::ZERO, block.default_state());
        for (_, transformer) in REGISTRY.block_transformers.iter() {
            for transform in &transformer.transforms {
                BlockStateProviderEvaluator::sample_block_state_provider_optional(
                    &level,
                    &REGISTRY,
                    &mut random,
                    &transform.block_state_provider,
                    BlockPos::ZERO,
                );
            }
        }
    }
    assert_eq!(random.next_i64(), untouched.next_i64());
}
