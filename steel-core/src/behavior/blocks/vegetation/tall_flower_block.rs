use steel_macros::block_behavior;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, DoublePlantBlock};

/// Vanilla `TallFlowerBlock` survival.
// TODO: Implement bonemeal and the rest of vanilla behavior.
#[block_behavior]
pub struct TallFlowerBlock {
    base: DoublePlantBlock,
}

impl TallFlowerBlock {
    /// Creates a new tall flower block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            base: DoublePlantBlock::new(block),
        }
    }
}

impl BlockBehavior for TallFlowerBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.base.can_survive(state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.base.get_state_for_placement(context)
    }
}
