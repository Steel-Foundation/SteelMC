//! `StructureVoidBlock` behavior (`net.minecraft.world.level.block.StructureVoidBlock`).
//!
//! Vanilla is invisible (`RenderShape.INVISIBLE`), tiny 6x6 shape, shade 1.0.
//! No server-side behavior.

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::BlockStateId;

use crate::behavior::{BlockBehavior, BlockPlaceContext};

/// Vanilla `StructureVoidBlock`.
#[block_behavior]
pub struct StructureVoidBlock {
    block: BlockRef,
}

impl StructureVoidBlock {
    /// Creates the behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for StructureVoidBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }
}
