use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::shapes::SupportType;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::fluid::get_fluid_state_from_block;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state};

/// Vanilla `SporeBlossomBlock` survival.
// TODO: Implement particles and the rest of vanilla behavior.
#[block_behavior]
pub struct SporeBlossomBlock {
    block: BlockRef,
}

impl SporeBlossomBlock {
    /// Creates a new spore blossom block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SporeBlossomBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let above_pos = pos.above();
        world.get_block_state(above_pos).is_face_sturdy_for_at(
            above_pos,
            Direction::Down,
            SupportType::Center,
        ) && get_fluid_state_from_block(world.get_block_state(pos)).is_empty()
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
