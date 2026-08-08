use steel_macros::block_behavior;
use steel_registry::blocks::properties::Direction;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::MultifaceBlock;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{BlockRef, default_surviving_state};

/// Vanilla `SculkVeinBlock` survival.
///
/// Inherits `canSurvive` from `MultifaceBlock`. Sculk-specific spread is left
/// as a TODO.
// TODO: Implement sculk spread, charge handling, and rotation/mirror overrides.
#[block_behavior]
pub struct SculkVeinBlock {
    block: BlockRef,
    multiface: MultifaceBlock,
}

impl SculkVeinBlock {
    /// Creates a new sculk vein block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            block,
            multiface: MultifaceBlock::new(block),
        }
    }
}

impl BlockBehavior for SculkVeinBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.multiface
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.multiface.can_survive(state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
