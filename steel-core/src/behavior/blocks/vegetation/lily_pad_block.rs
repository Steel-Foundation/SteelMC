use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::fluid::{FluidStateExt, get_fluid_state_from_block};
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state};

/// Vanilla `LilyPadBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct LilyPadBlock {
    block: BlockRef,
}

impl LilyPadBlock {
    /// Creates a new lily-pad block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for LilyPadBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below = world.get_block_state(below_pos);
        let below_fluid = get_fluid_state_from_block(below);
        let above_fluid = get_fluid_state_from_block(world.get_block_state(pos));

        (below_fluid.is_water() || below.get_block().has_tag(&BlockTag::SUPPORTS_LILY_PAD))
            && above_fluid.is_empty()
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
