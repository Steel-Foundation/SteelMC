//! Dropper block placement behavior.

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, EnumProperty};
use steel_utils::BlockStateId;

use crate::behavior::{BlockBehavior, BlockPlaceContext};

const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;

/// Vanilla dropper placement behavior.
#[block_behavior]
pub struct DropperBlock {
    block: BlockRef,
}

impl DropperBlock {
    /// Creates dropper behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for DropperBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(FACING, context.get_nearest_looking_direction().opposite()),
        )
    }
}
