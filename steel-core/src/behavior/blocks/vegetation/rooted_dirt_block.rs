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

    use super::*;

    struct RootedDirtLevel {
        min_y: i32,
        height: i32,
        below: BlockStateId,
    }

    impl RootedDirtLevel {
        fn new(min_y: i32, height: i32, below: BlockStateId) -> Self {
            Self {
                min_y,
                height,
                below,
            }
        }
    }

    impl LevelReader for RootedDirtLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            if pos == BlockPos::ZERO.below() {
                self.below
            } else {
                vanilla_blocks::AIR.default_state()
            }
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn min_y(&self) -> i32 {
            self.min_y
        }

        fn height(&self) -> i32 {
            self.height
        }
    }

    #[test]
    fn rooted_dirt_bonemeal_rejects_bottom_build_height() {
        init_test_registry();
        let behavior = RootedDirtBlock::new(&vanilla_blocks::ROOTED_DIRT);
        let state = vanilla_blocks::ROOTED_DIRT.default_state();
        let level = RootedDirtLevel::new(0, 1, vanilla_blocks::AIR.default_state());

        assert!(!behavior.is_valid_bonemeal_target(state, &level, BlockPos::ZERO));
    }

    #[test]
    fn rooted_dirt_bonemeal_accepts_in_bounds_air_below() {
        init_test_registry();
        let behavior = RootedDirtBlock::new(&vanilla_blocks::ROOTED_DIRT);
        let state = vanilla_blocks::ROOTED_DIRT.default_state();
        let level = RootedDirtLevel::new(-1, 2, vanilla_blocks::AIR.default_state());

        assert!(behavior.is_valid_bonemeal_target(state, &level, BlockPos::ZERO));
    }
}
