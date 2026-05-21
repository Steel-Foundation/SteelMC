use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::{REGISTRY, TaggedRegistryExt, vanilla_block_tags};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state};

/// Vanilla `MangrovePropaguleBlock` survival.
///
/// - Hanging: block above must be in `SUPPORTS_HANGING_MANGROVE_PROPAGULE`.
/// - Planted: block below must be in `SUPPORTS_MANGROVE_PROPAGULE` (vanilla's
///   `mayPlaceOn` override applied to the `VegetationBlock` survival rule).
// TODO: Implement growth ticking, bonemeal advance, and shape updates.
#[block_behavior]
pub struct MangrovePropaguleBlock {
    block: BlockRef,
}

impl MangrovePropaguleBlock {
    /// Creates a new mangrove propagule block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for MangrovePropaguleBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(&BlockStateProperties::HANGING) {
            let above = world.get_block_state(pos.above());
            return REGISTRY.blocks.is_in_tag(
                above.get_block(),
                &vanilla_block_tags::SUPPORTS_HANGING_MANGROVE_PROPAGULE_TAG,
            );
        }

        let below = world.get_block_state(pos.below());
        REGISTRY.blocks.is_in_tag(
            below.get_block(),
            &vanilla_block_tags::SUPPORTS_MANGROVE_PROPAGULE_TAG,
        )
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
