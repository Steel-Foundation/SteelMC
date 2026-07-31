//! Constant-strength redstone source block behavior.

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, SignalQueryContext};

/// Vanilla `PoweredBlock`, used by the redstone block.
#[block_behavior]
pub struct PoweredBlock {
    block: BlockRef,
}

impl PoweredBlock {
    /// Creates the constant-strength behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for PoweredBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn is_signal_source(&self, _state: BlockStateId, _context: SignalQueryContext) -> bool {
        true
    }

    fn get_own_signal(
        &self,
        _state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        15
    }
}
