//! Vanilla `TintedGlassBlock` behavior.

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::BlockStateId;

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;

/// Tinted glass placement behavior.
///
/// Its light dampening and skylight behavior are extracted
/// into block-state metadata.
#[block_behavior]
pub struct TintedGlassBlock {
    block: BlockRef,
}

impl TintedGlassBlock {
    /// Creates a tinted glass behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for TintedGlassBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }
}
