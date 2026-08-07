//! Vanilla `EndRodBlock` behavior.

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, EnumProperty};
use steel_utils::{BlockStateId, Direction};

use crate::behavior::{BlockBehavior, BlockPlaceContext};

const FACING: EnumProperty<Direction> = BlockStateProperties::FACING;

/// End rods use the clicked face for their orientation and reverse against an aligned rod.
#[block_behavior]
pub struct EndRodBlock {
    block: BlockRef,
}

impl EndRodBlock {
    /// Creates an end rod behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for EndRodBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let clicked_face = context.clicked_face();
        let adjacent_pos = context.place_pos().relative(clicked_face.opposite());
        let adjacent = context.world.get_block_state(adjacent_pos);
        let facing =
            if adjacent.get_block() == self.block && adjacent.get_value(&FACING) == clicked_face {
                clicked_face.opposite()
            } else {
                clicked_face
            };

        Some(self.block.default_state().set_value(&FACING, facing))
    }
}
