use steel_macros::block_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_block_tags::BlockTag,
};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext,
        blocks::vegetation::{
            Vegetation, default_surviving_state,
            vegetation_block::{vegetation_can_survive, vegetation_update_shape},
        },
    },
    world::{LevelReader, ScheduledTickAccess},
};

/// Behavior for azalea blocks.
#[block_behavior]
pub struct AzaleaBlock {
    block: BlockRef,
}

impl AzaleaBlock {
    /// Creates a new azalea block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for AzaleaBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        vegetation_update_shape(self, state, world, pos)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        vegetation_can_survive(self, state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}

impl Vegetation for AzaleaBlock {
    fn may_place_on(&self, state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        state.get_block().has_tag(&BlockTag::SUPPORTS_AZALEA)
    }
}
