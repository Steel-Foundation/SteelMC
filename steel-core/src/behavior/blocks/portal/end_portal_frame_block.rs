//! End portal frame block implementation.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::World;

/// Behavior for end portal frame blocks.
#[block_behavior]
pub struct EndPortalFrameBlock {
    block: BlockRef,
}

impl EndPortalFrameBlock {
    /// Creates a new end portal frame block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    const fn analog_output_signal(has_eye: bool) -> i32 {
        if has_eye { 15 } else { 0 }
    }
}

impl BlockBehavior for EndPortalFrameBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state().set_value(
            &BlockStateProperties::HORIZONTAL_FACING,
            context.horizontal_direction().opposite(),
        ))
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
    ) -> i32 {
        Self::analog_output_signal(state.get_value(&BlockStateProperties::EYE))
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::block_state_ext::BlockStateExt;
    use steel_registry::blocks::properties::BlockStateProperties;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::EndPortalFrameBlock;

    #[test]
    fn end_portal_frame_analog_output_depends_on_eye() {
        init_test_registry();

        let empty = vanilla_blocks::END_PORTAL_FRAME.default_state();
        let filled = empty.set_value(&BlockStateProperties::EYE, true);

        assert_eq!(
            EndPortalFrameBlock::analog_output_signal(empty.get_value(&BlockStateProperties::EYE)),
            0
        );
        assert_eq!(
            EndPortalFrameBlock::analog_output_signal(filled.get_value(&BlockStateProperties::EYE)),
            15
        );
    }
}
