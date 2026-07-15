use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::properties::Direction;
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{ScheduledTickAccess, World};

use super::{BlockRef, coral_scan_for_water, schedule_coral_die_tick};

/// Vanilla `CoralBlock` survival.
#[block_behavior]
pub struct CoralBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks, json = "dead_block")]
    dead_block: BlockRef,
}

impl CoralBlock {
    /// Creates a new live coral block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, dead_block: BlockRef) -> Self {
        Self { block, dead_block }
    }

    fn dead_state(&self) -> BlockStateId {
        self.dead_block.default_state()
    }
}

impl BlockBehavior for CoralBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_coral_die_tick(state, world, pos, self.block);

        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        schedule_coral_die_tick(state, context.world, context.place_pos(), self.block);
        Some(state)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !coral_scan_for_water(state, world, pos) {
            world.set_block(pos, self.dead_state(), UpdateFlags::UPDATE_CLIENTS);
        }
    }
}
