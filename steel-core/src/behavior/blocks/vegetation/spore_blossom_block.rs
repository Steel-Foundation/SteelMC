use steel_macros::block_behavior;
use steel_registry::blocks::shapes::SupportType;
use steel_registry::{blocks::properties::Direction, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::fluid::get_fluid_state_from_block;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{BlockRef, default_surviving_state};

/// Vanilla `SporeBlossomBlock` survival.
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
        world.is_face_sturdy_for(
            world.get_block_state(above_pos),
            above_pos,
            Direction::Down,
            SupportType::Center,
        ) && get_fluid_state_from_block(world.get_block_state(pos)).is_empty()
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction == Direction::Up && !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }
        state
    }
}
