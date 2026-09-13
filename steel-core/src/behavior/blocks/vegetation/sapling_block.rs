use std::sync::Arc;

use rand::{Rng, RngExt};
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, World};

use super::{BlockRef, TreeGrower, default_surviving_state, survives_on_tag};

const STAGE: &IntProperty = &BlockStateProperties::STAGE;
const MIN_GROWTH_LIGHT: u8 = 9;
const RANDOM_GROWTH_BOUND: u32 = 7;
const BONEMEAL_SUCCESS_CHANCE: f32 = 0.45;

/// Vanilla `SaplingBlock`.
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

    /// Vanilla `SaplingBlock.advanceTree`, shared with `MangrovePropaguleBlock`.
    pub(crate) fn advance_tree(
        tree_grower: TreeGrower,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        rng: &mut dyn Rng,
    ) {
        if state.get_value(STAGE) == 0 {
            world.set_block(pos, state.set_value(STAGE, 1), UpdateFlags::UPDATE_NONE);
        } else {
            tree_grower.grow_tree(world, pos, state, rng);
        }
    }

    /// Vanilla `SaplingBlock.isValidBonemealTarget`, shared with `MangrovePropaguleBlock`.
    pub(crate) fn is_valid_bonemeal_target_for(
        tree_grower: TreeGrower,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let height_offset = tree_grower.minimum_height().unwrap_or(0);
        !world.is_outside_build_height(pos.above_n(height_offset).y())
    }

    /// Vanilla `SaplingBlock.randomTick` growth gate: enough light above and a 1/7 roll.
    fn should_randomly_grow(
        world: &dyn LevelReader,
        pos: BlockPos,
        sky_darkening: u8,
        rng: &mut dyn Rng,
    ) -> bool {
        world.max_local_raw_brightness(pos.above(), sky_darkening) >= MIN_GROWTH_LIGHT
            && rng.random_range(0..RANDOM_GROWTH_BOUND) == 0
    }

    fn random_tick_with_rng(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        rng: &mut dyn Rng,
    ) {
        if Self::should_randomly_grow(world.as_ref(), pos, world.sky_darkening(), rng) {
            Self::advance_tree(self.tree_grower, world, pos, state, rng);
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
        Self::is_valid_bonemeal_target_for(self.tree_grower, world, pos)
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
        Self::advance_tree(self.tree_grower, world, pos, state, rng);
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{
        MaxRng, TestLevel, ZeroRng, fresh_test_world, insert_ready_full_chunk,
    };

    #[test]
    fn random_growth_requires_light_after_sky_darkening() {
        let pos = BlockPos::ZERO;
        let level = TestLevel::default().with_raw_brightness(MIN_GROWTH_LIGHT);

        assert!(SaplingBlock::should_randomly_grow(
            &level,
            pos,
            0,
            &mut ZeroRng
        ));
        assert!(!SaplingBlock::should_randomly_grow(
            &level,
            pos,
            1,
            &mut ZeroRng
        ));
        assert!(!SaplingBlock::should_randomly_grow(
            &level,
            pos,
            0,
            &mut MaxRng
        ));
    }

    #[test]
    fn first_growth_advances_stage_without_placing_a_tree() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("sapling_first_growth_stage");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::OAK_SAPLING.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
        let behavior = SaplingBlock::new(&vanilla_blocks::OAK_SAPLING, TreeGrower::Oak);

        behavior.perform_bonemeal(state, &world, &mut ZeroRng, pos);

        assert_eq!(world.get_block_state(pos), state.set_value(STAGE, 1));
    }

    #[test]
    fn bonemeal_success_and_height_limit_match_vanilla() {
        init_vanilla_registry();
        let world = fresh_test_world("sapling_bonemeal_chance");
        let behavior = SaplingBlock::new(&vanilla_blocks::OAK_SAPLING, TreeGrower::Oak);
        let state = vanilla_blocks::OAK_SAPLING.default_state();
        let level = TestLevel::default().with_min_y(0).with_height(10);
        let min_height = TreeGrower::Oak
            .minimum_height()
            .expect("oak has a primary tree");
        assert!(min_height > 0);

        assert!(behavior.is_bonemeal_success(state, &world, &mut ZeroRng, BlockPos::ZERO));
        assert!(!behavior.is_bonemeal_success(state, &world, &mut MaxRng, BlockPos::ZERO));
        assert!(behavior.is_valid_bonemeal_target(
            state,
            &level,
            BlockPos::new(0, 9 - min_height, 0)
        ));
        assert!(!behavior.is_valid_bonemeal_target(
            state,
            &level,
            BlockPos::new(0, 10 - min_height, 0)
        ));
    }

    #[test]
    fn mega_only_growers_accept_bonemeal_up_to_the_build_limit() {
        init_vanilla_registry();
        let behavior = SaplingBlock::new(&vanilla_blocks::DARK_OAK_SAPLING, TreeGrower::DarkOak);
        let state = vanilla_blocks::DARK_OAK_SAPLING.default_state();
        let level = TestLevel::default().with_min_y(0).with_height(10);

        assert!(behavior.is_valid_bonemeal_target(state, &level, BlockPos::new(0, 9, 0)));
        assert!(!behavior.is_valid_bonemeal_target(state, &level, BlockPos::new(0, 10, 0)));
    }
}
