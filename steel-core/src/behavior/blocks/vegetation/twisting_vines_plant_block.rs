use steel_macros::block_behavior;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state, growing_plant_can_survive};

/// Vanilla `TwistingVinesPlantBlock` (body) survival.
// TODO: Implement shape updates.
#[block_behavior]
pub struct TwistingVinesPlantBlock {
    block: BlockRef,
}

impl TwistingVinesPlantBlock {
    /// Creates a new twisting vines plant (body) block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for TwistingVinesPlantBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        growing_plant_can_survive(
            world,
            pos,
            Direction::Up,
            &vanilla_blocks::TWISTING_VINES,
            &vanilla_blocks::TWISTING_VINES_PLANT,
        )
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
