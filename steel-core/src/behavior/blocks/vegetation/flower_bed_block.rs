use steel_macros::block_behavior;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state, survives_on_tag};

/// Vanilla `FlowerBedBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct FlowerBedBlock {
    block: BlockRef,
}

impl FlowerBedBlock {
    /// Creates a new flower-bed block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for FlowerBedBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        survives_on_tag(world, pos, &BlockTag::SUPPORTS_VEGETATION)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
