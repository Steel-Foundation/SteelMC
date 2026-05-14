use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::BlockRef;

/// Vanilla `HangingRootsBlock` survival.
// TODO: Implement scheduled water-tick handoff on shape updates.
#[block_behavior]
pub struct HangingRootsBlock {
    block: BlockRef,
}

impl HangingRootsBlock {
    /// Creates a new hanging roots block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for HangingRootsBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        // Vanilla: the block above must be face-sturdy on its DOWN face.
        let above = world.get_block_state(pos.above());
        above.is_face_sturdy(Direction::Down)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        if !self.can_survive(state, context.world, context.relative_pos) {
            return None;
        }
        Some(state.set_value(
            &BlockStateProperties::WATERLOGGED,
            context.is_water_source(),
        ))
    }
}
