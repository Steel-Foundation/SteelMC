use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::properties::Direction;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockPlaceContext, Fallable};
use crate::world::{ScheduledTickAccess, World};

use super::FallingBlock;

/// Vanilla `Dragon Egg` behavior.
#[block_behavior]
pub struct DragonEggBlock {
    falling: FallingBlock,
}

impl DragonEggBlock {
    /// Creates the server-side behavior for the dragon egg
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            falling: FallingBlock::new(block),
        }
    }
}

impl Fallable for DragonEggBlock {}

impl BlockBehavior for DragonEggBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.falling.get_state_for_placement(context)
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
        BlockBehavior::tick(&self.falling, state, world, pos);
    }

    fn as_fallable(&self) -> Option<&dyn Fallable> {
        Some(self)
    }
}
