use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
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

    /// Creates vanilla's initial hanging propagule state.
    pub(crate) fn create_new_hanging_propagule() -> BlockStateId {
        vanilla_blocks::MANGROVE_PROPAGULE
            .default_state()
            .set_value(&BlockStateProperties::HANGING, true)
            .set_value(&BlockStateProperties::AGE_4, 0)
    }
}

impl BlockBehavior for MangrovePropaguleBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(&BlockStateProperties::HANGING) {
            let above = world.get_block_state(pos.above());
            return above
                .get_block()
                .has_tag(&BlockTag::SUPPORTS_HANGING_MANGROVE_PROPAGULE);
        }

        let below = world.get_block_state(pos.below());
        below
            .get_block()
            .has_tag(&BlockTag::SUPPORTS_MANGROVE_PROPAGULE)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use super::*;

    #[test]
    fn new_hanging_propagule_starts_at_age_zero() {
        init_test_registry();

        let state = MangrovePropaguleBlock::create_new_hanging_propagule();

        assert_eq!(state.get_block(), &vanilla_blocks::MANGROVE_PROPAGULE);
        assert!(state.get_value(&BlockStateProperties::HANGING));
        assert_eq!(state.get_value(&BlockStateProperties::AGE_4), 0);
    }
}
