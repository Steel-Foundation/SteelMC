use std::sync::Arc;

use rand::Rng;
use steel_macros::block_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, blocks::vegetation::bonemealable::Bonemealable},
    world::{LevelReader, World},
};

/// Vanilla `RootedDirtBlock` bonemeal behavior
#[block_behavior]
pub struct RootedDirtBlock {
    block: BlockRef,
}

impl RootedDirtBlock {
    /// Creates a new rooted dirt block
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for RootedDirtBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
impl Bonemealable for RootedDirtBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        world.get_block_state(pos.below()).is_air()
    }

    fn perform_bonemeal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        world.set_block(
            pos.below(),
            vanilla_blocks::HANGING_ROOTS.default_state(),
            UpdateFlags::UPDATE_ALL,
        );
    }
}
