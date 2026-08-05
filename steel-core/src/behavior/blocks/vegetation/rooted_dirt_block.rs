use std::sync::Arc;

use rand::Rng;
use steel_macros::block_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, blocks::vegetation::bonemealable::Bonemealable},
    world::{LevelReader, World},
};

/// Vanilla `RootedDirtBlock` bonemeal behavior
#[block_behavior]
pub struct RootedDirtBlock {
    block: BlockRef,
}

impl RootedDirtBlock {
    /// Creates a new rooted dirt block
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for RootedDirtBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
impl Bonemealable for RootedDirtBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let below_pos = pos.below();
        !world.is_outside_build_height(below_pos.y()) && world.get_block_state(below_pos).is_air()
    }

    fn perform_bonemeal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        world.set_block(
            pos.below(),
            vanilla_blocks::HANGING_ROOTS.default_state(),
            UpdateFlags::UPDATE_ALL,
        );
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use crate::test_support::TestLevel;

    use super::*;

    fn rooted_dirt_level(min_y: i32, height: i32, below: BlockStateId) -> TestLevel {
        TestLevel::default()
            .with_min_y(min_y)
            .with_height(height)
            .with_block(BlockPos::ZERO.below(), below)
    }

    #[test]
    fn rooted_dirt_bonemeal_rejects_bottom_build_height() {
        init_test_registry();
        let behavior = RootedDirtBlock::new(&vanilla_blocks::ROOTED_DIRT);
        let state = vanilla_blocks::ROOTED_DIRT.default_state();
        let level = rooted_dirt_level(0, 1, vanilla_blocks::AIR.default_state());

        assert!(!behavior.is_valid_bonemeal_target(state, &level, BlockPos::ZERO));
    }

    #[test]
    fn rooted_dirt_bonemeal_accepts_in_bounds_air_below() {
        init_test_registry();
        let behavior = RootedDirtBlock::new(&vanilla_blocks::ROOTED_DIRT);
        let state = vanilla_blocks::ROOTED_DIRT.default_state();
        let level = rooted_dirt_level(-1, 2, vanilla_blocks::AIR.default_state());

        assert!(behavior.is_valid_bonemeal_target(state, &level, BlockPos::ZERO));
    }
}
