use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::{BlockBehavior, schedule_water_tick_if_waterlogged};
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext};
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{BlockRef, default_surviving_state};

/// Vanilla `SeaPickleBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct SeaPickleBlock {
    block: BlockRef,
}

impl SeaPickleBlock {
    /// Creates a new sea pickle block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn may_place_on(world: &dyn LevelReader, state: BlockStateId, pos: BlockPos) -> bool {
        BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .get_collision_boxes(state, world, pos, BlockCollisionContext::empty())
            .iter()
            .any(|aabb| !aabb.is_empty() && aabb.max_y() >= 1.0)
            || world.is_face_sturdy(state, pos, Direction::Up)
    }
}

impl BlockBehavior for SeaPickleBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }

        schedule_water_tick_if_waterlogged(state, world, pos);
        state
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        Self::may_place_on(world, world.get_block_state(below_pos), below_pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::properties::BlockStateProperties;
    use steel_registry::{test_support::init_test_registry, vanilla_fluids};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;

    #[test]
    fn sea_pickle_checks_survival_before_scheduling_water() {
        init_test_registry();
        init_behaviors();
        let behavior = SeaPickleBlock::new(&vanilla_blocks::SEA_PICKLE);
        let state = vanilla_blocks::SEA_PICKLE
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);
        let pos = BlockPos::new(0, 64, 0);
        let unsupported = TestLevel::default();

        assert!(
            behavior
                .update_shape(
                    state,
                    &unsupported,
                    pos,
                    Direction::North,
                    pos.north(),
                    vanilla_blocks::AIR.default_state(),
                )
                .is_air()
        );
        assert!(unsupported.scheduled_fluid_ticks.borrow().is_empty());

        let supported =
            TestLevel::default().with_block(pos.below(), vanilla_blocks::STONE.default_state());
        assert_eq!(
            behavior.update_shape(
                state,
                &supported,
                pos,
                Direction::North,
                pos.north(),
                vanilla_blocks::AIR.default_state(),
            ),
            state
        );
        assert_eq!(
            supported
                .scheduled_fluid_ticks
                .borrow()
                .iter()
                .map(|tick| (tick.fluid, tick.delay))
                .collect::<Vec<_>>(),
            vec![(&vanilla_fluids::WATER, 5)]
        );
    }
}
