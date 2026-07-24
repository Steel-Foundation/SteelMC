//! Comparator-visible respawn-anchor charge state.
//!
//! Charging, respawning, and invalid-dimension explosions are separate block
//! mechanics. The analog contract is purely state-driven and complete here.

use steel_macros::block_behavior;
use steel_registry::blocks::{
    BlockRef,
    block_state_ext::BlockStateExt as _,
    properties::{BlockStateProperties, Direction},
};
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext},
    world::LevelReader,
};

/// Vanilla respawn-anchor behavior required by analog signal readers.
#[block_behavior]
pub struct RespawnAnchorBlock {
    block: BlockRef,
}

impl RespawnAnchorBlock {
    /// Creates respawn-anchor behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Vanilla `RespawnAnchorBlock.getScaledChargeLevel(state, 15)`.
    const fn analog_output(charges: i32) -> i32 {
        charges * 15 / 4
    }
}

impl BlockBehavior for RespawnAnchorBlock {
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
        Self::analog_output(i32::from(
            state.get_value(&BlockStateProperties::RESPAWN_ANCHOR_CHARGES),
        ))
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
    fn registered_respawn_anchor_scales_all_charge_levels_like_vanilla() {
        init_test_registry();
        init_behaviors();
        let level = TestLevel::default();
        let behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::RESPAWN_ANCHOR);

        for (charges, expected) in [0, 3, 7, 11, 15].into_iter().enumerate() {
            let state = vanilla_blocks::RESPAWN_ANCHOR.default_state().set_value(
                &BlockStateProperties::RESPAWN_ANCHOR_CHARGES,
                u8::try_from(charges).expect("charge fixture fits the block property"),
            );
            assert!(behavior.has_analog_output_signal(state));
            assert_eq!(
                behavior.get_analog_output_signal(state, &level, BlockPos::ZERO, Direction::North,),
                expected,
            );
        }
    }
}
