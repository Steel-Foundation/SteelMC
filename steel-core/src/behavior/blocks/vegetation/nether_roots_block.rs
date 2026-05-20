use steel_macros::block_behavior;
use steel_registry::{vanilla_block_tags, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state, survives_on_tag};

/// Vanilla `NetherRootsBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct NetherRootsBlock {
    block: BlockRef,
}

impl NetherRootsBlock {
    /// Creates a new nether roots block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn support_tag(&self) -> steel_utils::Identifier {
        if self.block == &vanilla_blocks::WARPED_ROOTS {
            vanilla_block_tags::SUPPORTS_WARPED_ROOTS_TAG
        } else {
            vanilla_block_tags::SUPPORTS_CRIMSON_ROOTS_TAG
        }
    }
}

impl BlockBehavior for NetherRootsBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let support_tag = self.support_tag();
        survives_on_tag(world, pos, &support_tag)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
