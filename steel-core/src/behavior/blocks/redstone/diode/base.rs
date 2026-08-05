//! Shared vanilla `DiodeBlock` behavior for repeaters and comparators.

use std::sync::Arc;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::shapes::SupportType;
use steel_registry::vanilla_blocks;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::behavior::BlockPlaceContext;
use crate::world::{
    LevelReader, SignalQueryContext, World, get_control_input_signal,
    get_signal as get_redstone_signal, tick_scheduler::TickPriority,
};

/// Common server-side behavior inherited from vanilla's abstract `DiodeBlock`.
pub(super) struct DiodeBlock {
    pub(super) block: BlockRef,
}

impl DiodeBlock {
    #[must_use]
    pub(super) const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    pub(super) fn can_survive_on(
        level: &dyn LevelReader,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> bool {
        level.is_face_sturdy_for(
            neighbor_state,
            neighbor_pos,
            Direction::Up,
            SupportType::Rigid,
        )
    }

    pub(super) fn can_survive(level: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        Self::can_survive_on(level, below_pos, level.get_block_state(below_pos))
    }

    pub(super) fn state_for_placement(&self, context: &BlockPlaceContext<'_>) -> BlockStateId {
        self.block.default_state().set_value(
            &BlockStateProperties::HORIZONTAL_FACING,
            context.horizontal_direction().opposite(),
        )
    }

    pub(super) fn get_input_signal(
        level: &dyn LevelReader,
        pos: BlockPos,
        state: BlockStateId,
    ) -> i32 {
        let direction = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let target_pos = pos.relative(direction);
        let input = get_redstone_signal(level, target_pos, direction, SignalQueryContext::DEFAULT);
        if input >= 15 {
            return input;
        }

        let target_state = level.get_block_state(target_pos);
        let wire_power = if target_state.get_block() == &vanilla_blocks::REDSTONE_WIRE {
            i32::from(target_state.get_value(&BlockStateProperties::POWER))
        } else {
            0
        };
        input.max(wire_power)
    }

    pub(super) fn get_alternate_signal(
        level: &dyn LevelReader,
        pos: BlockPos,
        state: BlockStateId,
        side_input_diodes_only: bool,
    ) -> i32 {
        let direction = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let clockwise = direction.rotate_y_clockwise();
        let counter_clockwise = direction.rotate_y_counter_clockwise();
        get_control_input_signal(
            level,
            pos.relative(clockwise),
            clockwise,
            side_input_diodes_only,
        )
        .max(get_control_input_signal(
            level,
            pos.relative(counter_clockwise),
            counter_clockwise,
            side_input_diodes_only,
        ))
    }

    pub(super) fn should_prioritize(
        level: &dyn LevelReader,
        pos: BlockPos,
        state: BlockStateId,
    ) -> bool {
        let direction = state
            .get_value(&BlockStateProperties::HORIZONTAL_FACING)
            .opposite();
        let opposite_state = level.get_block_state(pos.relative(direction));
        BLOCK_BEHAVIORS
            .get_behavior(opposite_state.get_block())
            .is_diode()
            && opposite_state.get_value(&BlockStateProperties::HORIZONTAL_FACING) != direction
    }

    pub(super) fn check_tick_on_neighbor(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        is_locked: bool,
        should_turn_on: bool,
        delay: i32,
    ) {
        if is_locked {
            return;
        }

        let powered = state.get_value(&BlockStateProperties::POWERED);
        if powered == should_turn_on || world.will_tick_block_this_tick(pos, self.block) {
            return;
        }

        let priority = Self::tick_priority(world.as_ref(), pos, state, powered);
        world.schedule_block_tick(pos, self.block, delay, priority);
    }

    pub(super) fn tick_priority(
        level: &dyn LevelReader,
        pos: BlockPos,
        state: BlockStateId,
        powered: bool,
    ) -> TickPriority {
        if Self::should_prioritize(level, pos, state) {
            TickPriority::ExtremelyHigh
        } else if powered {
            TickPriority::VeryHigh
        } else {
            TickPriority::High
        }
    }

    pub(super) fn tick(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        is_locked: bool,
        should_turn_on: bool,
        delay: i32,
    ) {
        if is_locked {
            return;
        }

        let powered = state.get_value(&BlockStateProperties::POWERED);
        if powered && !should_turn_on {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::POWERED, false),
                UpdateFlags::UPDATE_CLIENTS,
            );
        } else if !powered {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::POWERED, true),
                UpdateFlags::UPDATE_CLIENTS,
            );
            if !should_turn_on {
                world.schedule_block_tick(pos, self.block, delay, TickPriority::VeryHigh);
            }
        }
    }

    pub(super) fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        on_supported: impl FnOnce(),
    ) {
        if world.get_block_state(pos).get_block() != self.block {
            return;
        }
        if Self::can_survive(world.as_ref(), pos) {
            on_supported();
            return;
        }

        world.drop_resources(state, pos);
        world.remove_block(pos, false);
        for direction in Direction::ALL {
            world.update_neighbors_at(pos.relative(direction), self.block);
        }
    }

    pub(super) fn set_placed_by(&self, world: &Arc<World>, pos: BlockPos, should_turn_on: bool) {
        if should_turn_on {
            world.schedule_block_tick_default(pos, self.block, 1);
        }
    }

    pub(super) fn on_place(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.update_neighbors_in_front(world, pos, state);
    }

    pub(super) fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        if !moved_by_piston {
            self.update_neighbors_in_front(world, pos, state);
        }
    }

    pub(super) fn update_neighbors_in_front(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) {
        let direction = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let opposite_pos = pos.relative(direction.opposite());
        world.neighbor_changed(opposite_pos, self.block);
        world.update_neighbors_at_except_from_facing(opposite_pos, self.block, direction);
    }

    pub(super) fn own_signal(state: BlockStateId, output_signal: i32) -> i32 {
        if state.get_value(&BlockStateProperties::POWERED) {
            output_signal
        } else {
            0
        }
    }

    pub(super) fn signal(state: BlockStateId, direction: Direction, output_signal: i32) -> i32 {
        if state.get_value(&BlockStateProperties::HORIZONTAL_FACING) == direction {
            Self::own_signal(state, output_signal)
        } else {
            0
        }
    }
}
