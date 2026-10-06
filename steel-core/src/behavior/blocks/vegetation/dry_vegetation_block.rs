use steel_macros::block_behavior;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{
    BlockRef, default_surviving_state, survives_on_tag, vegetation_block::survival_update_shape,
};

/// Vanilla `DryVegetationBlock` behavior
#[block_behavior]
pub struct DryVegetationBlock {
    block: BlockRef,
}

impl DryVegetationBlock {
    /// Creates a new dry-vegetation block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for DryVegetationBlock {
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
        survives_on_tag(world, pos, &BlockTag::SUPPORTS_DRY_VEGETATION)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
