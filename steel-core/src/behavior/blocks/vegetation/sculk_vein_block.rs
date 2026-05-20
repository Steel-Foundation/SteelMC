use steel_macros::block_behavior;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state, multiface_can_survive};

/// Vanilla `SculkVeinBlock` survival.
///
/// Inherits `canSurvive` from `MultifaceBlock`. Sculk-specific spread is left
/// as a TODO.
// TODO: Implement sculk spread, charge handling, and rotation/mirror overrides.
#[block_behavior]
pub struct SculkVeinBlock {
    block: BlockRef,
}

impl SculkVeinBlock {
    /// Creates a new sculk vein block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SculkVeinBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        multiface_can_survive(state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
