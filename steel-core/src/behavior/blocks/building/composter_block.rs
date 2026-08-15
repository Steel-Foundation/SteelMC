//! Comparator-visible composter state.
//!
//! Composting interactions and hopper automation are independent mechanics;
//! comparators read the block-state fill level directly in vanilla.

use steel_macros::block_behavior;
use steel_registry::blocks::{
    BlockRef,
    block_state_ext::BlockStateExt as _,
    properties::{BlockStateProperties, Direction, IntProperty},
};
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext},
    entity::ai::path::PathComputationType,
    world::World,
};

/// Vanilla composter behavior required by analog signal readers.
#[block_behavior]
pub struct ComposterBlock {
    block: BlockRef,
}

const LEVEL_COMPOSTER: &IntProperty = &BlockStateProperties::LEVEL_COMPOSTER;

impl ComposterBlock {
    /// Creates composter behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for ComposterBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        state: BlockStateId,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        i32::from(state.get_value(LEVEL_COMPOSTER))
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_blocks};

    use super::*;
    use crate::{
        behavior::{BLOCK_BEHAVIORS, init_behaviors},
        test_support::fresh_test_world,
    };

    #[test]
    fn registered_composter_outputs_its_full_state_level() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("composter_analog_output");
        let state = vanilla_blocks::COMPOSTER
            .default_state()
            .set_value(LEVEL_COMPOSTER, 8);
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());

        assert!(behavior.has_analog_output_signal(state));
        assert_eq!(
            behavior.get_analog_output_signal(
                state,
                world.as_ref(),
                BlockPos::ZERO,
                Direction::North,
            ),
            8,
        );
    }
}
