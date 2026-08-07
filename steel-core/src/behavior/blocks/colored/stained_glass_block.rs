//! Vanilla `StainedGlassBlock` behavior.

use steel_macros::block_behavior;
use steel_registry::DyeColor;
use steel_registry::blocks::BlockRef;
use steel_utils::BlockStateId;

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;

/// Colored full-block glass behavior.
#[block_behavior]
pub struct StainedGlassBlock {
    block: BlockRef,
    #[json_arg(
        r#enum = "DyeColor",
        json = "color",
        module = "steel_registry::dye_color"
    )]
    #[expect(unused, reason = "Stored for beacon beam color")]
    color: DyeColor,
}

impl StainedGlassBlock {
    /// Creates a stained glass behavior for the given color.
    #[must_use]
    pub const fn new(block: BlockRef, color: DyeColor) -> Self {
        Self { block, color }
    }
}

impl BlockBehavior for StainedGlassBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }
}
