use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, coral_plant_can_survive};

/// Vanilla `BaseCoralFanBlock` survival (dead coral fans).
///
/// Same `canSurvive` as `BaseCoralPlantTypeBlock`.
#[block_behavior]
pub struct BaseCoralFanBlock {
    block: BlockRef,
}

impl BaseCoralFanBlock {
    /// Creates a new dead coral fan block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for BaseCoralFanBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        coral_plant_can_survive(world, pos)
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
