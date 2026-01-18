//! End portal frame block implementation.

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_utils::BlockStateId;

use crate::behavior::block::BlockBehaviour;
use crate::behavior::context::BlockPlaceContext;

/// Behavior for end portal frame blocks.
pub struct EndPortalFrameBlock {
    block: BlockRef,
}

impl EndPortalFrameBlock {
    /// Creates a new end portal frame block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for EndPortalFrameBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state().set_value(
            &BlockStateProperties::HORIZONTAL_FACING,
            context.horizontal_direction.opposite(),
        ))
    }
}
