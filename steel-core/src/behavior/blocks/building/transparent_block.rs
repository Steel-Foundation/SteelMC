//! `TransparentBlock` behavior (`net.minecraft.world.level.block.TransparentBlock`).
//!
//! Vanilla `TransparentBlock` (glass) overrides `getVisualShape` (empty),
//! `getShadeBrightness` (1.0) and `propagatesSkylightDown` (true). No
//! server-side behavior.

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::BlockStateId;

use crate::behavior::{BlockBehavior, BlockPlaceContext};

/// Vanilla `TransparentBlock` (glass).
#[block_behavior]
pub struct TransparentBlock {
    block: BlockRef,
}

impl TransparentBlock {
    /// Creates the behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for TransparentBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }
}
