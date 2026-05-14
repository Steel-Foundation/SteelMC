use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, coral_wall_fan_can_survive};

/// Vanilla `CoralWallFanBlock` survival (live coral wall fans).
///
/// Inherits `canSurvive` from `BaseCoralWallFanBlock`. Death tick is left as a
/// TODO.
// TODO: Implement death tick and scheduled tick on placement.
#[block_behavior]
pub struct CoralWallFanBlock {
    block: BlockRef,
}

impl CoralWallFanBlock {
    /// Creates a new live coral wall fan block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for CoralWallFanBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        coral_wall_fan_can_survive(world, pos, facing)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let facing = context.horizontal_direction.opposite();
        let state = self
            .block
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
            .set_value(
                &BlockStateProperties::WATERLOGGED,
                context.is_water_source(),
            );
        self.can_survive(state, context.world, context.relative_pos)
            .then_some(state)
    }
}
