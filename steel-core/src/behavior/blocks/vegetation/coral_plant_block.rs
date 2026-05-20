use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, coral_plant_can_survive};

/// Vanilla `CoralPlantBlock` survival (live coral plants such as `tube_coral`).
///
/// Inherits `canSurvive` from `BaseCoralPlantTypeBlock`. Death tick (converting
/// to the dead variant when no surrounding water) is left as a TODO.
// TODO: Implement death tick, scheduled tick on placement, and water update.
#[block_behavior]
pub struct CoralPlantBlock {
    block: BlockRef,
}

impl CoralPlantBlock {
    /// Creates a new live coral plant block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for CoralPlantBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        coral_plant_can_survive(world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        if !self.can_survive(state, context.world, context.relative_pos) {
            return None;
        }
        // Vanilla: WATERLOGGED reflects whether the click position has a full
        // water source.
        Some(state.set_value(
            &BlockStateProperties::WATERLOGGED,
            context.is_water_source(),
        ))
    }
}
