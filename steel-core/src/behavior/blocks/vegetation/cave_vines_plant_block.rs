use steel_macros::block_behavior;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state, growing_plant_can_survive};

/// Vanilla `CaveVinesPlantBlock` (body) survival.
// TODO: Implement berry interactions and shape updates.
#[block_behavior]
pub struct CaveVinesPlantBlock {
    block: BlockRef,
}

impl CaveVinesPlantBlock {
    /// Creates a new cave vines plant (body) block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for CaveVinesPlantBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        growing_plant_can_survive(
            world,
            pos,
            Direction::Down,
            &vanilla_blocks::CAVE_VINES,
            &vanilla_blocks::CAVE_VINES_PLANT,
        )
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
