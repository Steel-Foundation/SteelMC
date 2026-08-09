use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::properties::Direction;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockPlaceContext, Fallable};
use crate::world::{ScheduledTickAccess, World};

use super::FallingBlock;

/// Vanilla `ColoredFallingBlock` behavior.
#[block_behavior]
pub struct ColoredFallingBlock {
    falling: FallingBlock,
    #[json_arg(value)]
    dust_color_rgba: i32,
}

impl ColoredFallingBlock {
    /// Creates a colored falling block from extracted class data.
    #[must_use]
    pub const fn new(block: BlockRef, dust_color_rgba: i32) -> Self {
        Self {
            falling: FallingBlock::new(block),
            dust_color_rgba,
        }
    }

    /// Returns the extracted falling-dust color used by vanilla clients.
    #[must_use]
    pub const fn dust_color_rgba(&self) -> i32 {
        self.dust_color_rgba
    }
}

impl Fallable for ColoredFallingBlock {}

impl BlockBehavior for ColoredFallingBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.falling.block().default_state())
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        self.falling.on_place(world, pos);
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        ticks: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.falling.update_shape(state, ticks, pos)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let _ = FallingBlock::tick(state, world, pos);
    }

    fn as_fallable(&self) -> Option<&dyn Fallable> {
        Some(self)
    }
}
