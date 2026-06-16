use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state};

/// Vanilla `LeafLitterBlock` uses sturdy top-face support, not the vegetation tag.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct LeafLitterBlock {
    block: BlockRef,
}

impl LeafLitterBlock {
    /// Creates a new leaf-litter block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for LeafLitterBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        world
            .get_block_state(below_pos)
            .is_face_sturdy_at(below_pos, Direction::Up)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
