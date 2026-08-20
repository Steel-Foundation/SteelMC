use std::sync::Arc;

use rand::{Rng, RngExt};
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::feature::{ConfiguredFeature, ConfiguredFeatureKind, TrunkPlacer};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{REGISTRY, vanilla_blocks, vanilla_configured_features};
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};
use steel_worldgen::biomes::obfuscate_biome_seed;

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, World};
use crate::worldgen::feature::FeatureDecorationRunner;

use super::{BlockRef, default_surviving_state, survives_on_tag};

const STAGE: &IntProperty = &BlockStateProperties::STAGE;
const RANDOM_GROWTH_BOUND: u32 = 7;
const MIN_GROWTH_LIGHT: u8 = 9;
const BONEMEAL_SUCCESS_CHANCE: f32 = 0.45;

/// Vanilla configured tree selection for a sapling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreeGrower {
    /// Oak tree selection.
    Oak,
    /// Spruce tree selection.
    Spruce,
    /// Birch tree selection.
    Birch,
    /// Jungle tree selection.
    Jungle,
    /// Acacia tree selection.
    Acacia,
    /// Cherry tree selection.
    Cherry,
    /// Dark oak tree selection.
    DarkOak,
    /// Pale oak tree selection.
    PaleOak,
}

impl TreeGrower {
    fn primary_tree(self) -> Option<&'static ConfiguredFeature> {
        Some(match self {
            Self::Oak => &vanilla_configured_features::OAK,
            Self::Spruce => &vanilla_configured_features::SPRUCE,
            Self::Birch => &vanilla_configured_features::BIRCH,
            Self::Jungle => &vanilla_configured_features::JUNGLE_TREE_NO_VINE,
            Self::Acacia => &vanilla_configured_features::ACACIA,
            Self::Cherry => &vanilla_configured_features::CHERRY,
            Self::DarkOak | Self::PaleOak => return None,
        })
    }

    fn tree(self, rng: &mut dyn Rng, has_flowers: bool) -> Option<&'static ConfiguredFeature> {
        Some(match self {
            Self::Oak => {
                let fancy = rng.random::<f32>() < 0.1;
                match (fancy, has_flowers) {
                    (true, true) => &vanilla_configured_features::FANCY_OAK_BEES_005,
                    (true, false) => &vanilla_configured_features::FANCY_OAK,
                    (false, true) => &vanilla_configured_features::OAK_BEES_005,
                    (false, false) => &vanilla_configured_features::OAK,
                }
            }
            Self::Spruce => &vanilla_configured_features::SPRUCE,
            Self::Birch if has_flowers => &vanilla_configured_features::BIRCH_BEES_005,
            Self::Birch => &vanilla_configured_features::BIRCH,
            Self::Jungle => &vanilla_configured_features::JUNGLE_TREE_NO_VINE,
            Self::Acacia => &vanilla_configured_features::ACACIA,
            Self::Cherry if has_flowers => &vanilla_configured_features::CHERRY_BEES_005,
            Self::Cherry => &vanilla_configured_features::CHERRY,
            Self::DarkOak | Self::PaleOak => return None,
        })
    }

    fn mega_tree(self, rng: &mut dyn Rng) -> Option<&'static ConfiguredFeature> {
        Some(match self {
            Self::Spruce if rng.random::<f32>() < 0.5 => &vanilla_configured_features::MEGA_PINE,
            Self::Spruce => &vanilla_configured_features::MEGA_SPRUCE,
            Self::Jungle => &vanilla_configured_features::MEGA_JUNGLE_TREE,
            Self::DarkOak => &vanilla_configured_features::DARK_OAK,
            Self::PaleOak => &vanilla_configured_features::PALE_OAK_BONEMEAL,
            Self::Oak | Self::Birch | Self::Acacia | Self::Cherry => return None,
        })
    }

    fn minimum_height(self) -> i32 {
        let Some(feature) = self.primary_tree() else {
            return 0;
        };
        let ConfiguredFeatureKind::Tree(config) = &feature.kind else {
            return 0;
        };

        match &config.trunk_placer {
            TrunkPlacer::Straight(base)
            | TrunkPlacer::Giant(base)
            | TrunkPlacer::Fancy(base)
            | TrunkPlacer::Forking(base)
            | TrunkPlacer::DarkOak(base)
            | TrunkPlacer::MegaJungle(base) => base.base_height,
            TrunkPlacer::Bending(placer) => placer.base_height,
            TrunkPlacer::UpwardsBranching(placer) => placer.base_height,
            TrunkPlacer::Cherry(placer) => placer.base_height,
        }
    }

    fn has_flowers(world: &dyn LevelReader, pos: BlockPos) -> bool {
        for x in -2..=2 {
            for y in -1..=1 {
                for z in -2..=2 {
                    if world
                        .get_block_state(pos.offset(x, y, z))
                        .get_block()
                        .has_tag(&BlockTag::FLOWERS)
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn place_tree(
        world: &Arc<World>,
        pos: BlockPos,
        feature: &ConfiguredFeature,
        rng: &mut dyn Rng,
    ) -> bool {
        let ConfiguredFeatureKind::Tree(config) = &feature.kind else {
            return false;
        };
        let mut level = Arc::clone(world);
        let mut worldgen_random = WorldgenRandom::from_seed(rng.random());
        FeatureDecorationRunner::place_tree_feature(
            &mut level,
            &REGISTRY,
            &mut worldgen_random,
            config,
            pos,
            obfuscate_biome_seed(world.seed()),
        )
    }

    fn is_two_by_two(state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let block = state.get_block();
        world.get_block_state(pos).get_block() == block
            && world.get_block_state(pos.east()).get_block() == block
            && world.get_block_state(pos.south()).get_block() == block
            && world.get_block_state(pos.south().east()).get_block() == block
    }

    fn grow_mega_tree(
        self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        rng: &mut dyn Rng,
    ) -> Option<bool> {
        let feature = self.mega_tree(rng)?;
        for offset_x in [0, -1] {
            for offset_z in [0, -1] {
                let origin = pos.offset(offset_x, 0, offset_z);
                if !Self::is_two_by_two(state, world, origin) {
                    continue;
                }

                let positions = [origin, origin.east(), origin.south(), origin.south().east()];
                for sapling_pos in positions {
                    world.set_block(
                        sapling_pos,
                        vanilla_blocks::AIR.default_state(),
                        UpdateFlags::UPDATE_NONE,
                    );
                }
                if Self::place_tree(world, origin, feature, rng) {
                    return Some(true);
                }
                for sapling_pos in positions {
                    world.set_block(sapling_pos, state, UpdateFlags::UPDATE_NONE);
                }
                return Some(false);
            }
        }
        None
    }

    fn grow_tree(
        self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        rng: &mut dyn Rng,
    ) -> bool {
        if let Some(placed) = self.grow_mega_tree(world, pos, state, rng) {
            return placed;
        }

        let Some(feature) = self.tree(rng, Self::has_flowers(world, pos)) else {
            return false;
        };
        let replacement = vanilla_blocks::AIR.default_state();
        world.set_block(pos, replacement, UpdateFlags::UPDATE_NONE);
        if Self::place_tree(world, pos, feature, rng) {
            if world.get_block_state(pos) == replacement {
                world.send_block_updated(pos);
            }
            true
        } else {
            world.set_block(pos, state, UpdateFlags::UPDATE_NONE);
            false
        }
    }
}

/// Vanilla `SaplingBlock` behavior.
#[block_behavior]
pub struct SaplingBlock {
    block: BlockRef,
    #[json_arg(r#enum = "TreeGrower", json = "tree_grower_name")]
    tree_grower: TreeGrower,
}

impl SaplingBlock {
    /// Creates a new sapling block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, tree_grower: TreeGrower) -> Self {
        Self { block, tree_grower }
    }

    fn advance_tree(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        rng: &mut dyn Rng,
    ) {
        if state.get_value(STAGE) == 0 {
            world.set_block(pos, state.set_value(STAGE, 1), UpdateFlags::UPDATE_NONE);
        } else {
            self.tree_grower.grow_tree(world, pos, state, rng);
        }
    }

    fn should_randomly_grow(world: &dyn LevelReader, pos: BlockPos, rng: &mut dyn Rng) -> bool {
        world.max_local_raw_brightness(pos.above(), 0) >= MIN_GROWTH_LIGHT
            && rng.random_range(0..RANDOM_GROWTH_BOUND) == 0
    }

    fn random_tick_with_rng(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        rng: &mut dyn Rng,
    ) {
        if Self::should_randomly_grow(world, pos, rng) {
            self.advance_tree(world, pos, state, rng);
        }
    }
}

impl BlockBehavior for SaplingBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        survives_on_tag(world, pos, &BlockTag::SUPPORTS_VEGETATION)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.random_tick_with_rng(state, world, pos, &mut rand::rng());
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for SaplingBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        !world.is_outside_build_height(pos.above_n(self.tree_grower.minimum_height()).y())
    }

    fn is_bonemeal_success(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        rng: &mut dyn Rng,
        _pos: BlockPos,
    ) -> bool {
        rng.random::<f32>() < BONEMEAL_SUCCESS_CHANCE
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        self.advance_tree(world, pos, state, rng);
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use rand::{SeedableRng, TryRng, rngs::StdRng};
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{TestLevel, fresh_test_world, insert_ready_full_chunk};

    #[derive(Default)]
    struct ZeroRng;

    impl TryRng for ZeroRng {
        type Error = Infallible;

        fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
            Ok(0)
        }

        fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
            Ok(0)
        }

        fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
            dst.fill(0);
            Ok(())
        }
    }

    #[derive(Default)]
    struct MaxRng;

    impl TryRng for MaxRng {
        type Error = Infallible;

        fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
            Ok(u32::MAX)
        }

        fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
            Ok(u64::MAX)
        }

        fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
            dst.fill(u8::MAX);
            Ok(())
        }
    }

    fn assert_feature(actual: Option<&ConfiguredFeature>, expected: &ConfiguredFeature) {
        assert_eq!(actual.map(|feature| &feature.key), Some(&expected.key));
    }

    fn prepared_tree_world(key: &'static str) -> (Arc<World>, BlockPos) {
        let world = fresh_test_world(key);
        let pos = BlockPos::new(8, 64, 8);
        let center = ChunkPos::from_block_pos(pos);
        for chunk_x in center.0.x - 2..=center.0.x + 2 {
            for chunk_z in center.0.y - 2..=center.0.y + 2 {
                insert_ready_full_chunk(&world, ChunkPos::new(chunk_x, chunk_z));
            }
        }
        for x in pos.x() - 20..=pos.x() + 20 {
            for z in pos.z() - 20..=pos.z() + 20 {
                assert!(world.set_block(
                    BlockPos::new(x, pos.y() - 1, z),
                    vanilla_blocks::DIRT.default_state(),
                    UpdateFlags::UPDATE_NONE,
                ));
            }
        }
        (world, pos)
    }

    #[test]
    fn configured_tree_selection_matches_vanilla() {
        init_vanilla_registry();

        assert_feature(
            TreeGrower::Oak.tree(&mut ZeroRng, false),
            &vanilla_configured_features::FANCY_OAK,
        );
        assert_feature(
            TreeGrower::Oak.tree(&mut MaxRng, false),
            &vanilla_configured_features::OAK,
        );
        assert_feature(
            TreeGrower::Oak.tree(&mut ZeroRng, true),
            &vanilla_configured_features::FANCY_OAK_BEES_005,
        );
        assert_feature(
            TreeGrower::Birch.tree(&mut ZeroRng, true),
            &vanilla_configured_features::BIRCH_BEES_005,
        );
        assert_feature(
            TreeGrower::Cherry.tree(&mut ZeroRng, true),
            &vanilla_configured_features::CHERRY_BEES_005,
        );
        assert_feature(
            TreeGrower::Spruce.mega_tree(&mut ZeroRng),
            &vanilla_configured_features::MEGA_PINE,
        );
        assert_feature(
            TreeGrower::Spruce.mega_tree(&mut MaxRng),
            &vanilla_configured_features::MEGA_SPRUCE,
        );
        assert_feature(
            TreeGrower::Jungle.mega_tree(&mut ZeroRng),
            &vanilla_configured_features::MEGA_JUNGLE_TREE,
        );
        assert_feature(
            TreeGrower::DarkOak.mega_tree(&mut ZeroRng),
            &vanilla_configured_features::DARK_OAK,
        );
        assert_feature(
            TreeGrower::PaleOak.mega_tree(&mut ZeroRng),
            &vanilla_configured_features::PALE_OAK_BONEMEAL,
        );
        assert!(TreeGrower::DarkOak.tree(&mut ZeroRng, false).is_none());
        assert!(TreeGrower::PaleOak.tree(&mut ZeroRng, false).is_none());
    }

    #[test]
    fn random_growth_requires_vanilla_light_level() {
        let pos = BlockPos::ZERO;
        let dark = TestLevel::default().with_raw_brightness(MIN_GROWTH_LIGHT - 1);
        let bright = TestLevel::default().with_raw_brightness(MIN_GROWTH_LIGHT);

        assert!(!SaplingBlock::should_randomly_grow(
            &dark,
            pos,
            &mut ZeroRng
        ));
        assert!(SaplingBlock::should_randomly_grow(
            &bright,
            pos,
            &mut ZeroRng
        ));
    }

    #[test]
    fn nearby_flowers_select_bee_tree_features() {
        init_vanilla_registry();
        let pos = BlockPos::ZERO;
        let nearby = TestLevel::default().with_block(
            pos.offset(2, 1, -2),
            vanilla_blocks::DANDELION.default_state(),
        );
        let outside = TestLevel::default().with_block(
            pos.offset(3, 0, 0),
            vanilla_blocks::DANDELION.default_state(),
        );

        assert!(TreeGrower::has_flowers(&nearby, pos));
        assert!(!TreeGrower::has_flowers(&outside, pos));
    }

    #[test]
    fn first_growth_advance_sets_stage_one() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("sapling_first_growth_stage");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::OAK_SAPLING.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
        let behavior = SaplingBlock::new(&vanilla_blocks::OAK_SAPLING, TreeGrower::Oak);

        behavior.perform_bonemeal(state, &world, &mut ZeroRng, pos);

        assert_eq!(world.get_block_state(pos).get_value(STAGE), 1);
    }

    #[test]
    fn bonemeal_chance_and_height_target_match_vanilla() {
        init_vanilla_registry();
        let world = fresh_test_world("sapling_bonemeal_chance");
        let behavior = SaplingBlock::new(&vanilla_blocks::OAK_SAPLING, TreeGrower::Oak);
        let state = vanilla_blocks::OAK_SAPLING.default_state();
        let height = TreeGrower::Oak.minimum_height();
        let level = TestLevel::default().with_min_y(0).with_height(10);

        assert!(behavior.as_bonemealable().is_some());
        assert!(behavior.is_bonemeal_success(state, &world, &mut ZeroRng, BlockPos::ZERO));
        assert!(!behavior.is_bonemeal_success(state, &world, &mut MaxRng, BlockPos::ZERO));
        assert!(behavior.is_valid_bonemeal_target(state, &level, BlockPos::new(0, 9 - height, 0),));
        assert!(!behavior.is_valid_bonemeal_target(
            state,
            &level,
            BlockPos::new(0, 10 - height, 0),
        ));
    }

    #[test]
    fn failed_mega_tree_restores_all_four_saplings() {
        init_vanilla_registry();
        init_behaviors();
        let (world, pos) = prepared_tree_world("sapling_failed_mega_restore");
        let state = vanilla_blocks::DARK_OAK_SAPLING
            .default_state()
            .set_value(STAGE, 1);
        let positions = [pos, pos.east(), pos.south(), pos.south().east()];
        for sapling_pos in positions {
            assert!(world.set_block(sapling_pos, state, UpdateFlags::UPDATE_NONE));
        }
        for x in pos.x() - 2..=pos.x() + 3 {
            for z in pos.z() - 2..=pos.z() + 3 {
                assert!(world.set_block(
                    BlockPos::new(x, pos.y() + 1, z),
                    vanilla_blocks::STONE.default_state(),
                    UpdateFlags::UPDATE_NONE,
                ));
            }
        }

        assert!(!TreeGrower::DarkOak.grow_tree(&world, pos, state, &mut ZeroRng));
        for sapling_pos in positions {
            assert_eq!(world.get_block_state(sapling_pos), state);
        }
    }

    #[test]
    fn all_sapling_growers_generate_their_tree() {
        init_vanilla_registry();
        init_behaviors();
        let variants = [
            (
                "sapling_oak_tree",
                &vanilla_blocks::OAK_SAPLING,
                TreeGrower::Oak,
                &vanilla_blocks::OAK_LOG,
                false,
            ),
            (
                "sapling_spruce_tree",
                &vanilla_blocks::SPRUCE_SAPLING,
                TreeGrower::Spruce,
                &vanilla_blocks::SPRUCE_LOG,
                false,
            ),
            (
                "sapling_birch_tree",
                &vanilla_blocks::BIRCH_SAPLING,
                TreeGrower::Birch,
                &vanilla_blocks::BIRCH_LOG,
                false,
            ),
            (
                "sapling_jungle_tree",
                &vanilla_blocks::JUNGLE_SAPLING,
                TreeGrower::Jungle,
                &vanilla_blocks::JUNGLE_LOG,
                false,
            ),
            (
                "sapling_acacia_tree",
                &vanilla_blocks::ACACIA_SAPLING,
                TreeGrower::Acacia,
                &vanilla_blocks::ACACIA_LOG,
                false,
            ),
            (
                "sapling_cherry_tree",
                &vanilla_blocks::CHERRY_SAPLING,
                TreeGrower::Cherry,
                &vanilla_blocks::CHERRY_LOG,
                false,
            ),
            (
                "sapling_dark_oak_tree",
                &vanilla_blocks::DARK_OAK_SAPLING,
                TreeGrower::DarkOak,
                &vanilla_blocks::DARK_OAK_LOG,
                true,
            ),
            (
                "sapling_pale_oak_tree",
                &vanilla_blocks::PALE_OAK_SAPLING,
                TreeGrower::PaleOak,
                &vanilla_blocks::PALE_OAK_LOG,
                true,
            ),
        ];

        for (key, sapling, grower, expected_log, two_by_two) in variants {
            let (world, pos) = prepared_tree_world(key);
            let state = sapling.default_state().set_value(STAGE, 1);
            let positions = [pos, pos.east(), pos.south(), pos.south().east()];
            let sapling_count = if two_by_two { positions.len() } else { 1 };
            for sapling_pos in positions.into_iter().take(sapling_count) {
                assert!(world.set_block(sapling_pos, state, UpdateFlags::UPDATE_NONE));
            }

            let placed = (0..128).any(|seed| {
                let mut rng = StdRng::seed_from_u64(seed);
                grower.grow_tree(&world, pos, state, &mut rng)
            });
            assert!(placed, "{key} should generate a tree");
            assert!(
                (pos.x() - 12..=pos.x() + 12).any(|x| {
                    (pos.z() - 12..=pos.z() + 12).any(|z| {
                        (pos.y()..=pos.y() + 40).any(|y| {
                            world.get_block_state(BlockPos::new(x, y, z)).get_block()
                                == expected_log
                        })
                    })
                }),
                "{key} should place its matching logs"
            );
        }
    }
}
