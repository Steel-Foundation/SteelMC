use steel_macros::block_behavior;
use steel_registry::blocks::properties::Direction;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::MultifaceBlock;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::BlockRef;

/// Vanilla `GlowLichenBlock` survival.
///
/// Inherits `canSurvive` from `MultifaceBlock`. Subclass-specific spread and
/// bonemeal behavior is left as a TODO.
// TODO: Implement spread, bonemeal, and rotation/mirror overrides.
#[block_behavior]
pub struct GlowLichenBlock {
    _block: BlockRef,
    multiface: MultifaceBlock,
}

impl GlowLichenBlock {
    /// Creates a new glow lichen block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            _block: block,
            multiface: MultifaceBlock::new(block),
        }
    }
}

impl BlockBehavior for GlowLichenBlock {
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
        self.multiface.get_state_for_placement(context)
    }

    fn can_be_replaced(&self, state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        self.multiface.can_be_replaced(state, context)
    }
}
