use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::{REGISTRY, TaggedRegistryExt, vanilla_block_tags, vanilla_fluid_tags};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::BlockStateBehaviorExt;
use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state};

/// Vanilla `SugarCaneBlock` survival.
///
/// Survives if the block below is sugar cane, or it is in
/// `BlockTags.SUPPORTS_SUGAR_CANE` and at least one of the four horizontal
/// neighbors of the block below is in `BlockTags.SUPPORTS_SUGAR_CANE_ADJACENTLY`
/// or has a fluid in `FluidTags.SUPPORTS_SUGAR_CANE_ADJACENTLY` (i.e. water).
// TODO: Implement age growth and shape updates that break the cane.
#[block_behavior]
pub struct SugarCaneBlock {
    block: BlockRef,
}

impl SugarCaneBlock {
    /// Creates a new sugar cane block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SugarCaneBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below = world.get_block_state(below_pos);
        let below_block = below.get_block();

        if below_block == self.block {
            return true;
        }

        if !REGISTRY
            .blocks
            .is_in_tag(below_block, &vanilla_block_tags::SUPPORTS_SUGAR_CANE_TAG)
        {
            return false;
        }

        for direction in Direction::HORIZONTAL {
            let neighbor_pos = below_pos.relative(direction);
            let neighbor_state = world.get_block_state(neighbor_pos);

            if REGISTRY.blocks.is_in_tag(
                neighbor_state.get_block(),
                &vanilla_block_tags::SUPPORTS_SUGAR_CANE_ADJACENTLY_TAG,
            ) {
                return true;
            }

            let fluid = neighbor_state.get_fluid_state();
            if REGISTRY.fluids.is_in_tag(
                fluid.fluid_id,
                &vanilla_fluid_tags::SUPPORTS_SUGAR_CANE_ADJACENTLY_TAG,
            ) {
                return true;
            }
        }

        false
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
