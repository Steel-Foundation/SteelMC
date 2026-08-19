use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::BlockStateId;

use crate::behavior::{BlockBehavior, BlockPlaceContext};

/// Behavior for crying obsidian blocks.
#[block_behavior]
pub struct CryingObsidianBlock {
    block: BlockRef,
}

impl CryingObsidianBlock {
    /// Creates a crying obsidian block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for CryingObsidianBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    // TODO: Implement animateTick function: crying obsidian tear particles
}
