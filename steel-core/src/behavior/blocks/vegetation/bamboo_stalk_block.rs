use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_block_tags;
use steel_registry::{REGISTRY, TaggedRegistryExt};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state};

/// Vanilla `BambooStalkBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct BambooStalkBlock {
    block: BlockRef,
}

impl BambooStalkBlock {
    /// Creates a new bamboo stalk block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for BambooStalkBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below = world.get_block_state(pos.below());
        REGISTRY
            .blocks
            .is_in_tag(below.get_block(), &vanilla_block_tags::SUPPORTS_BAMBOO_TAG)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
