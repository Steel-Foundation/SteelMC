use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, can_attach_to_multiface, default_surviving_state};

/// Vanilla `HangingMossBlock` survival (e.g. `pale_hanging_moss`).
// TODO: Implement TIP property update on shape changes, bonemeal growth, and tick.
#[block_behavior]
pub struct HangingMossBlock {
    block: BlockRef,
}

impl HangingMossBlock {
    /// Creates a new hanging moss block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for HangingMossBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        // Vanilla `canStayAtPosition`: the block above either attaches via the
        // multiface rule (support OR collision face full) or is more hanging
        // moss of the same kind.
        let above_pos = pos.above();
        if can_attach_to_multiface(world, above_pos, Direction::Up) {
            return true;
        }
        world.get_block_state(above_pos).get_block() == self.block
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
