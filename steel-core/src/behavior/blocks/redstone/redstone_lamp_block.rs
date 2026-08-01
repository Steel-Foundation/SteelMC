//! Vanilla redstone lamp behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::world::{SignalGetter as _, World};

const TURN_OFF_DELAY: i32 = 4;

/// Vanilla `RedstoneLampBlock`, including its delayed turn-off edge.
#[block_behavior]
pub struct RedstoneLampBlock {
    block: BlockRef,
}

impl RedstoneLampBlock {
    /// Creates redstone-lamp behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for RedstoneLampBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state().set_value(
            &BlockStateProperties::LIT,
            context.world.has_neighbor_signal(context.place_pos()),
        ))
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        let lit = state.get_value(&BlockStateProperties::LIT);
        if lit == world.has_neighbor_signal(pos) {
            return;
        }
        if lit {
            world.schedule_block_tick_default(pos, self.block, TURN_OFF_DELAY);
        } else {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::LIT, true),
                UpdateFlags::UPDATE_CLIENTS,
            );
        }
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if state.get_value(&BlockStateProperties::LIT) && !world.has_neighbor_signal(pos) {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::LIT, false),
                UpdateFlags::UPDATE_CLIENTS,
            );
        }
    }
}
