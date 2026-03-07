use steel_registry::{
    REGISTRY,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::{
    behavior::{BlockBehaviour, BlockPlaceContext, weathering::next_copper_stage},
    world::World,
};

/// Block Behavior for blocks that can weather/oxidize
pub struct WeatheringBlock {
    block: BlockRef,
}

impl WeatheringBlock {
    /// Creates a new `WeatheringBlock` behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for WeatheringBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state()) // dont know if this is correct
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        true
    }

    fn random_tick(&self, state: BlockStateId, world: &World, pos: BlockPos) {
        let old_block = state.get_block();

        if let Some(new_block) = next_copper_stage(old_block) {
            world.set_block(
                pos,
                REGISTRY.blocks.copy_matching_properties(state, new_block), // this needs testing
                UpdateFlags::UPDATE_ALL,
            );
        }
    }
}
