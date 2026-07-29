//! Vanilla observer behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::world::{LevelReader, ScheduledTickAccess, SignalQueryContext, World};

const PULSE_DELAY: i32 = 2;
const PLACEMENT_RESET_FLAGS: UpdateFlags =
    UpdateFlags::UPDATE_CLIENTS.union(UpdateFlags::UPDATE_KNOWN_SHAPE);

/// Vanilla `ObserverBlock` two-game-tick pulse behavior.
#[block_behavior]
pub struct ObserverBlock {
    block: BlockRef,
}

impl ObserverBlock {
    /// Creates observer behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn start_signal(&self, ticks: &dyn ScheduledTickAccess, pos: BlockPos) {
        if !ticks.has_scheduled_block_tick(pos, self.block) {
            ticks.schedule_block_tick_default(pos, self.block, PULSE_DELAY);
        }
    }

    fn update_neighbors_in_front(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let direction = state.get_value(&BlockStateProperties::FACING);
        let output_pos = pos.relative(direction.opposite());
        world.neighbor_changed(output_pos, self.block);
        world.update_neighbors_at_except_from_facing(output_pos, self.block, direction);
    }

    fn own_signal(state: BlockStateId) -> i32 {
        if state.get_value(&BlockStateProperties::POWERED) {
            15
        } else {
            0
        }
    }
}

impl BlockBehavior for ObserverBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state().set_value(
            &BlockStateProperties::FACING,
            context.get_nearest_looking_direction(),
        ))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if state.get_value(&BlockStateProperties::FACING) == direction
            && !state.get_value(&BlockStateProperties::POWERED)
        {
            self.start_signal(world, pos);
        }
        state
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if state.get_value(&BlockStateProperties::POWERED) {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::POWERED, false),
                UpdateFlags::UPDATE_CLIENTS,
            );
        } else {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::POWERED, true),
                UpdateFlags::UPDATE_CLIENTS,
            );
            world.schedule_block_tick_default(pos, self.block, PULSE_DELAY);
        }
        self.update_neighbors_in_front(world, pos, state);
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if state.get_block() == old_state.get_block()
            || !state.get_value(&BlockStateProperties::POWERED)
            || world.has_scheduled_block_tick(pos, self.block)
        {
            return;
        }

        let reset_state = state.set_value(&BlockStateProperties::POWERED, false);
        world.set_block(pos, reset_state, PLACEMENT_RESET_FLAGS);
        self.update_neighbors_in_front(world, pos, reset_state);
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _moved_by_piston: bool,
    ) {
        if state.get_value(&BlockStateProperties::POWERED)
            && world.has_scheduled_block_tick(pos, self.block)
        {
            self.update_neighbors_in_front(
                world,
                pos,
                state.set_value(&BlockStateProperties::POWERED, false),
            );
        }
    }

    fn is_signal_source(&self, _state: BlockStateId, _context: SignalQueryContext) -> bool {
        true
    }

    fn get_own_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        Self::own_signal(state)
    }

    fn get_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        direction: Direction,
        _context: SignalQueryContext,
    ) -> i32 {
        if state.get_value(&BlockStateProperties::FACING) == direction {
            Self::own_signal(state)
        } else {
            0
        }
    }

    fn get_direct_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
        context: SignalQueryContext,
    ) -> i32 {
        self.get_signal(state, world, pos, direction, context)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;

    use super::*;
    use crate::test_support::TestLevel;

    fn observer_state(facing: Direction, powered: bool) -> BlockStateId {
        vanilla_blocks::OBSERVER
            .default_state()
            .set_value(&BlockStateProperties::FACING, facing)
            .set_value(&BlockStateProperties::POWERED, powered)
    }

    #[test]
    fn observed_face_update_schedules_one_two_tick_pulse() {
        init_test_registry();
        let observer = ObserverBlock::new(&vanilla_blocks::OBSERVER);
        let pos = BlockPos::new(0, 64, 0);
        let state = observer_state(Direction::East, false);
        let level = TestLevel::default();

        observer.update_shape(
            state,
            &level,
            pos,
            Direction::East,
            pos.east(),
            vanilla_blocks::STONE.default_state(),
        );
        let scheduled = level.scheduled_block_ticks.borrow();
        assert_eq!(scheduled.len(), 1);
        assert_eq!(scheduled[0].pos, pos);
        assert_eq!(scheduled[0].block, &vanilla_blocks::OBSERVER);
        assert_eq!(scheduled[0].delay, PULSE_DELAY);
    }

    #[test]
    fn observer_output_is_powered_and_directional() {
        init_test_registry();
        let observer = ObserverBlock::new(&vanilla_blocks::OBSERVER);
        let state = observer_state(Direction::Down, true);
        let level = TestLevel::default();
        let pos = BlockPos::new(0, 64, 0);

        assert_eq!(
            observer.get_signal(
                state,
                &level,
                pos,
                Direction::Down,
                SignalQueryContext::DEFAULT,
            ),
            15
        );
        assert_eq!(
            observer.get_signal(
                state,
                &level,
                pos,
                Direction::Up,
                SignalQueryContext::DEFAULT,
            ),
            0
        );
    }
}
