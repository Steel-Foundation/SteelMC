use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::{REGISTRY, TaggedRegistryExt, vanilla_block_tags, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state};

/// Vanilla `BigDripleafStemBlock` survival.
///
/// Below must be stem or in `SUPPORTS_BIG_DRIPLEAF`; above must be stem or big
/// dripleaf head.
// TODO: Implement scheduled break on shape update and tick.
#[block_behavior]
pub struct BigDripleafStemBlock {
    block: BlockRef,
}

impl BigDripleafStemBlock {
    /// Creates a new big dripleaf stem block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for BigDripleafStemBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below = world.get_block_state(pos.below());
        let below_block = below.get_block();
        let below_ok = below_block == self.block
            || REGISTRY
                .blocks
                .is_in_tag(below_block, &vanilla_block_tags::SUPPORTS_BIG_DRIPLEAF_TAG);
        if !below_ok {
            return false;
        }

        let above = world.get_block_state(pos.above());
        let above_block = above.get_block();
        above_block == self.block || above_block == &vanilla_blocks::BIG_DRIPLEAF
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
