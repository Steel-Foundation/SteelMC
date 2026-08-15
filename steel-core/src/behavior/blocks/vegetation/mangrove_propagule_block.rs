use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, IntProperty,
};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::{BlockBehavior, schedule_water_tick_if_waterlogged};
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{BlockRef, default_surviving_state};

/// Vanilla `MangrovePropaguleBlock` survival.
///
/// - Hanging: block above must be in `SUPPORTS_HANGING_MANGROVE_PROPAGULE`.
/// - Planted: block below must be in `SUPPORTS_MANGROVE_PROPAGULE` (vanilla's
///   `mayPlaceOn` override applied to the `VegetationBlock` survival rule).
// TODO: Implement growth ticking and bonemeal advance.
#[block_behavior]
pub struct MangrovePropaguleBlock {
    block: BlockRef,
}

const AGE_4: &IntProperty = &BlockStateProperties::AGE_4;
const HANGING: &BoolProperty = &BlockStateProperties::HANGING;

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
            .set_value(HANGING, true)
            .set_value(AGE_4, 0)
    }
}

impl BlockBehavior for MangrovePropaguleBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);
        if self.can_survive(state, world, pos) {
            state
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(HANGING) {
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
    use steel_registry::init_vanilla_registry;

    use super::*;
    use crate::test_support::TestLevel;

    const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

    #[test]
    fn new_hanging_propagule_starts_at_age_zero() {
        init_vanilla_registry();

        let state = MangrovePropaguleBlock::create_new_hanging_propagule();

        assert_eq!(state.get_block(), &vanilla_blocks::MANGROVE_PROPAGULE);
        assert!(state.get_value(HANGING));
        assert_eq!(state.get_value(AGE_4), 0);
    }

    #[test]
    fn unsupported_waterlogged_propagule_schedules_water_before_breaking() {
        init_vanilla_registry();
        let behavior = MangrovePropaguleBlock::new(&vanilla_blocks::MANGROVE_PROPAGULE);
        let state = vanilla_blocks::MANGROVE_PROPAGULE
            .default_state()
            .set_value(WATERLOGGED, true);
        let level = TestLevel::default();

        assert!(
            behavior
                .update_shape(
                    state,
                    &level,
                    BlockPos::ZERO,
                    Direction::Down,
                    BlockPos::ZERO.below(),
                    vanilla_blocks::AIR.default_state(),
                )
                .is_air()
        );
        assert!(level.scheduled_water_tick());
    }
}
