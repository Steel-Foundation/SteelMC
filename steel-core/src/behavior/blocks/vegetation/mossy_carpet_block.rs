use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state};

/// Vanilla `MossyCarpetBlock` survival.
// TODO: Implement spreading, bonemeal, and the rest of vanilla behavior.
#[block_behavior]
pub struct MossyCarpetBlock {
    block: BlockRef,
}

impl MossyCarpetBlock {
    /// Creates a new mossy-carpet block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for MossyCarpetBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(&BlockStateProperties::BOTTOM) {
            !world.get_block_state(pos.below()).is_air()
        } else {
            let below = world.get_block_state(pos.below());
            below.get_block() == &vanilla_blocks::PALE_MOSS_CARPET
                && below.get_value(&BlockStateProperties::BOTTOM)
        }
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
