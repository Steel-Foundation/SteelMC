use steel_macros::block_behavior;
use steel_registry::DyeColor;
use steel_registry::blocks::BlockRef;
use steel_utils::BlockStateId;

use crate::behavior::{BlockBehavior, BlockPlaceContext};

/// All colored stained glass blocks.
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

    /// Returns the block's dye color, matching vanilla beacon beam metadata.
    #[must_use]
    pub const fn color(&self) -> DyeColor {
        self.color
    }
}

impl BlockBehavior for StainedGlassBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }
}
