//! State-driven cauldron behavior needed by comparators.
//!
//! Cauldron item interactions and drip filling remain separate block mechanics;
//! this module implements the complete vanilla analog-output contract shared by
//! empty, water, and powder-snow cauldrons.

use steel_macros::block_behavior;
use steel_registry::blocks::{
    BlockRef,
    block_state_ext::BlockStateExt as _,
    properties::{BlockStateProperties, Direction},
};
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext},
    entity::ai::path::PathComputationType,
    world::LevelReader,
};

/// Vanilla empty cauldron behavior.
#[block_behavior]
pub struct CauldronBlock {
    block: BlockRef,
}

impl CauldronBlock {
    /// Creates empty cauldron behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for CauldronBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        0
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}

/// Vanilla layered water and powder-snow cauldron behavior.
#[block_behavior]
pub struct LayeredCauldronBlock {
    block: BlockRef,
}

impl LayeredCauldronBlock {
    /// Creates layered cauldron behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for LayeredCauldronBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        i32::from(state.get_value(&BlockStateProperties::LEVEL_CAULDRON))
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
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::*;
    use crate::{
        behavior::{BLOCK_BEHAVIORS, init_behaviors},
        test_support::TestLevel,
    };

    #[test]
    fn registered_cauldron_behaviors_expose_vanilla_fill_levels() {
        init_test_registry();
        init_behaviors();
        let level = TestLevel::default();
        let pos = BlockPos::ZERO;

        let empty = vanilla_blocks::CAULDRON.default_state();
        let empty_behavior = BLOCK_BEHAVIORS.get_behavior(empty.get_block());
        assert!(empty_behavior.has_analog_output_signal(empty));
        assert_eq!(
            empty_behavior.get_analog_output_signal(empty, &level, pos, Direction::North),
            0,
        );

        for level_value in 1..=3 {
            let state = vanilla_blocks::WATER_CAULDRON
                .default_state()
                .set_value(&BlockStateProperties::LEVEL_CAULDRON, level_value);
            let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
            assert_eq!(
                behavior.get_analog_output_signal(state, &level, pos, Direction::North),
                i32::from(level_value),
            );
        }
    }
}
