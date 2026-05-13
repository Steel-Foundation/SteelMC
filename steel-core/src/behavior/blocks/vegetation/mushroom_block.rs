use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::{REGISTRY, TaggedRegistryExt, vanilla_block_tags};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state};

/// Vanilla `MushroomBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct MushroomBlock {
    block: BlockRef,
}

impl MushroomBlock {
    /// Creates a new mushroom block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for MushroomBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below = world.get_block_state(below_pos);
        if REGISTRY.blocks.is_in_tag(
            below.get_block(),
            &vanilla_block_tags::OVERRIDES_MUSHROOM_LIGHT_REQUIREMENT_TAG,
        ) {
            return true;
        }

        // TODO: Include vanilla's raw-brightness < 13 guard once LevelReader exposes light.
        below.is_solid()
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
