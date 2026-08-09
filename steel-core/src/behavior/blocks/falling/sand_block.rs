use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::properties::Direction;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockPlaceContext, Fallable};
use crate::world::{ScheduledTickAccess, World};

use super::ColoredFallingBlock;

/// Vanilla `SandBlock` behavior.
///
/// Its ambient desert sound uses client-local `playLocalSound`.
#[block_behavior]
pub struct SandBlock {
    #[json_arg(value, json = "dust_color_rgba")]
    colored: ColoredFallingBlock,
}

impl SandBlock {
    /// Creates sand behavior from extracted class data.
    #[must_use]
    pub const fn new(block: BlockRef, dust_color_rgba: i32) -> Self {
        Self {
            colored: ColoredFallingBlock::new(block, dust_color_rgba),
        }
    }
}

impl Fallable for SandBlock {}

impl BlockBehavior for SandBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.colored.get_state_for_placement(context)
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        moved_by_piston: bool,
    ) {
        self.colored
            .on_place(state, world, pos, old_state, moved_by_piston);
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        ticks: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.colored
            .update_shape(state, ticks, pos, direction, neighbor_pos, neighbor_state)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.colored.tick(state, world, pos);
    }

    fn as_fallable(&self) -> Option<&dyn Fallable> {
        Some(self)
    }
}
