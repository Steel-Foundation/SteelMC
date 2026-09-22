//! Stained glass block behavior implementation.
//!
//! Stained glass is a plain transparent block whose only behavior is tinting a beacon beam
//! that passes through it.

use steel_macros::block_behavior;
use steel_registry::DyeColor;
use steel_registry::blocks::BlockRef;
use steel_utils::BlockStateId;

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;

/// All solid colored glass blocks.
///
/// Vanilla parity: `StainedGlassBlock`.
#[block_behavior]
pub struct StainedGlassBlock {
    block: BlockRef,
    #[json_arg(
        r#enum = "DyeColor",
        json = "color",
        module = "steel_registry::dye_color"
    )]
    color: DyeColor,
}

impl StainedGlassBlock {
    /// Creates a new stained glass block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef, color: DyeColor) -> Self {
        Self { block, color }
    }
}

impl BlockBehavior for StainedGlassBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn beacon_beam_color(&self, _state: BlockStateId) -> Option<DyeColor> {
        Some(self.color)
    }
}
