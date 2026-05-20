use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::{REGISTRY, TaggedRegistryExt, vanilla_block_tags, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::BlockRef;

/// Vanilla `BigDripleafBlock` survival.
///
/// Survives if the block below is big dripleaf (self), big dripleaf stem, or
/// in the `SUPPORTS_BIG_DRIPLEAF` tag.
// TODO: Implement tilt-on-stand, projectile tilt, bonemeal stem growth.
#[block_behavior]
pub struct BigDripleafBlock {
    block: BlockRef,
}

impl BigDripleafBlock {
    /// Creates a new big dripleaf block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for BigDripleafBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below = world.get_block_state(pos.below());
        let below_block = below.get_block();
        below_block == self.block
            || below_block == &vanilla_blocks::BIG_DRIPLEAF_STEM
            || REGISTRY
                .blocks
                .is_in_tag(below_block, &vanilla_block_tags::SUPPORTS_BIG_DRIPLEAF_TAG)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        self.can_survive(state, context.world, context.relative_pos)
            .then_some(state.set_value(
                &BlockStateProperties::WATERLOGGED,
                context.is_water_source(),
            ))
    }
}
