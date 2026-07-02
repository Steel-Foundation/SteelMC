use steel_macros::block_behavior;
use steel_registry::{vanilla_block_tags::BlockTag, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{
    BlockRef, default_surviving_state, survives_on_tag, vegetation_block::survival_update_shape,
};

/// Vanilla `NetherRootsBlock` survival.
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
            BlockTag::SUPPORTS_WARPED_ROOTS
        } else {
            BlockTag::SUPPORTS_CRIMSON_ROOTS
        }
    }
}

impl BlockBehavior for NetherRootsBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        survival_update_shape(self, state, world, pos)
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let support_tag = self.support_tag();
        survives_on_tag(world, pos, &support_tag)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
