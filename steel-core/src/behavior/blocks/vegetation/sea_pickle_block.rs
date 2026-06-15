use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state};

/// Vanilla `SeaPickleBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct SeaPickleBlock {
    block: BlockRef,
}

impl SeaPickleBlock {
    /// Creates a new sea pickle block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn may_place_on(state: BlockStateId) -> bool {
        state
            .get_collision_shape()
            .iter()
            .any(|aabb| !aabb.is_empty() && aabb.max_y() >= 1.0)
            || state.is_face_sturdy(Direction::Up)
    }
}

impl BlockBehavior for SeaPickleBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        Self::may_place_on(world.get_block_state(pos.below()))
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
