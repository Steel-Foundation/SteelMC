use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, DoubleBlockHalf};
use steel_registry::{vanilla_block_tags, vanilla_blocks};
use steel_utils::math::Axis;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{BlockRef, default_surviving_state, survives_on_tag};

/// Vanilla `DoublePlantBlock` lower/upper-half survival.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct DoublePlantBlock {
    block: BlockRef,
}

impl DoublePlantBlock {
    /// Creates a new double-plant block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for DoublePlantBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let half = state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF);
        let neighbor_is_matching_other_half = neighbor_state.get_block() == self.block
            && neighbor_state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) != half;

        if direction.get_axis() == Axis::Y
            && (half == DoubleBlockHalf::Lower) == (direction == Direction::Up)
            && !neighbor_is_matching_other_half
        {
            return vanilla_blocks::AIR.default_state();
        }

        if half == DoubleBlockHalf::Lower
            && direction == Direction::Down
            && !self.can_survive(state, world, pos)
        {
            return vanilla_blocks::AIR.default_state();
        }

        state
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Upper {
            let below = world.get_block_state(pos.below());
            return below.get_block() == self.block
                && below.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF)
                    == DoubleBlockHalf::Lower;
        }

        survives_on_tag(world, pos, &vanilla_block_tags::SUPPORTS_VEGETATION_TAG)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if context.relative_pos.y() >= context.world.max_y_exclusive() - 1 {
            return None;
        }
        if !context
            .world
            .get_block_state(context.relative_pos.above())
            .is_replaceable()
        {
            return None;
        }
        default_surviving_state(self.block, self, context)
    }
}
