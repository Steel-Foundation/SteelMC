use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::ScheduledTickAccess;

use super::SnowyBlock;

/// Vanilla `GrassBlock` — snowy-top behavior only for now.
#[block_behavior]
pub struct GrassBlock {
    snowy: SnowyBlock,
}

impl GrassBlock {
    /// Creates a grass block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            snowy: SnowyBlock::new(block),
        }
    }
}

impl BlockBehavior for GrassBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.snowy.state_for_placement(context))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        _world: &dyn ScheduledTickAccess,
        _pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        SnowyBlock::update_shape(state, direction, neighbor_state)
    }
}
