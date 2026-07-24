//! Vanilla tripwire-hook line resolution and redstone output.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::{sound_events, vanilla_blocks, vanilla_game_events};
use steel_utils::axis::Axis;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockPlaceContext, PlacementSource};
use crate::world::game_event_context::GameEventContext;
use crate::world::{LevelReader, ScheduledTickAccess, SignalQueryContext, World};

const WIRE_DISTANCE_MAX: usize = 42;
const RECHECK_PERIOD: i32 = 10;

/// Vanilla `TripWireHookBlock` behavior.
#[block_behavior]
pub struct TripWireHookBlock {
    block: BlockRef,
}

impl TripWireHookBlock {
    /// Creates tripwire-hook behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn notify_neighbors(block: BlockRef, world: &Arc<World>, pos: BlockPos, direction: Direction) {
        let front = direction.opposite();
        // Experimental redstone orientations are intentionally omitted.
        world.update_neighbors_at(pos, block);
        world.update_neighbors_at(pos.relative(front), block);
    }

    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "booleans mirror vanilla's before/after tripwire state transition"
    )]
    fn emit_state(
        world: &Arc<World>,
        pos: BlockPos,
        attached: bool,
        powered: bool,
        was_attached: bool,
        was_powered: bool,
    ) {
        let (sound, pitch, event) = if powered && !was_powered {
            (
                &sound_events::BLOCK_TRIPWIRE_CLICK_ON,
                0.6,
                &vanilla_game_events::BLOCK_ACTIVATE,
            )
        } else if !powered && was_powered {
            (
                &sound_events::BLOCK_TRIPWIRE_CLICK_OFF,
                0.5,
                &vanilla_game_events::BLOCK_DEACTIVATE,
            )
        } else if attached && !was_attached {
            (
                &sound_events::BLOCK_TRIPWIRE_ATTACH,
                0.7,
                &vanilla_game_events::BLOCK_ATTACH,
            )
        } else if !attached && was_attached {
            (
                &sound_events::BLOCK_TRIPWIRE_DETACH,
                1.2 / rand::random::<f32>().mul_add(0.2, 0.9),
                &vanilla_game_events::BLOCK_DETACH,
            )
        } else {
            return;
        };
        world.play_block_sound(sound, pos, 0.4, pitch, None);
        world.game_event(event, pos, &GameEventContext::default());
    }

    pub(super) fn calculate_state(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        is_being_destroyed: bool,
        can_update: bool,
        wire_source: i32,
        wire_source_state: Option<BlockStateId>,
    ) {
        let direction = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let was_attached = state.get_value(&BlockStateProperties::ATTACHED);
        let was_powered = state.get_value(&BlockStateProperties::POWERED);
        let block = state.get_block();
        let mut attached = !is_being_destroyed;
        let mut powered = false;
        let mut receiver_distance = 0_usize;
        let mut wire_states = [None; WIRE_DISTANCE_MAX];

        for (distance, slot) in wire_states.iter_mut().enumerate().skip(1) {
            let test_pos = pos.relative_n(direction, distance as i32);
            let mut wire_state = world.get_block_state(test_pos);
            if wire_state.get_block() == &vanilla_blocks::TRIPWIRE_HOOK {
                if wire_state.get_value(&BlockStateProperties::HORIZONTAL_FACING)
                    == direction.opposite()
                {
                    receiver_distance = distance;
                }
                break;
            }

            if wire_state.get_block() != &vanilla_blocks::TRIPWIRE && distance as i32 != wire_source
            {
                attached = false;
                continue;
            }

            if distance as i32 == wire_source
                && let Some(source_state) = wire_source_state
            {
                wire_state = source_state;
            }
            let wire_armed = !wire_state.get_value(&BlockStateProperties::DISARMED);
            let wire_powered = wire_state.get_value(&BlockStateProperties::POWERED);
            powered |= wire_armed && wire_powered;
            *slot = Some(wire_state);
            if distance as i32 == wire_source {
                world.schedule_block_tick_default(pos, block, RECHECK_PERIOD);
                attached &= wire_armed;
            }
        }

        attached &= receiver_distance > 1;
        powered &= attached;
        let new_state = block
            .default_state()
            .set_value(&BlockStateProperties::ATTACHED, attached)
            .set_value(&BlockStateProperties::POWERED, powered);

        if receiver_distance > 0 {
            let receiver_pos = pos.relative_n(direction, receiver_distance as i32);
            let opposite = direction.opposite();
            world.set_block(
                receiver_pos,
                new_state.set_value(&BlockStateProperties::HORIZONTAL_FACING, opposite),
                UpdateFlags::UPDATE_ALL,
            );
            Self::notify_neighbors(block, world, receiver_pos, opposite);
            if world.get_block_state(pos).get_block() != &vanilla_blocks::TRIPWIRE_HOOK {
                Self::on_removed(new_state, world, pos);
                return;
            }
            Self::emit_state(
                world,
                receiver_pos,
                attached,
                powered,
                was_attached,
                was_powered,
            );
        }

        Self::emit_state(world, pos, attached, powered, was_attached, was_powered);
        if !is_being_destroyed {
            world.set_block(
                pos,
                new_state.set_value(&BlockStateProperties::HORIZONTAL_FACING, direction),
                UpdateFlags::UPDATE_ALL,
            );
            if can_update {
                Self::notify_neighbors(block, world, pos, direction);
            }
        }

        if was_attached != attached {
            for (distance, wire_state) in wire_states
                .iter()
                .enumerate()
                .take(receiver_distance)
                .skip(1)
            {
                let Some(wire_state) = wire_state else {
                    continue;
                };
                let test_pos = pos.relative_n(direction, distance as i32);
                let live_state = world.get_block_state(test_pos);
                if live_state.get_block() == &vanilla_blocks::TRIPWIRE
                    || live_state.get_block() == &vanilla_blocks::TRIPWIRE_HOOK
                {
                    world.set_block(
                        test_pos,
                        wire_state.set_value(&BlockStateProperties::ATTACHED, attached),
                        UpdateFlags::UPDATE_ALL,
                    );
                }
            }
        }
    }

    fn on_removed(state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let attached = state.get_value(&BlockStateProperties::ATTACHED);
        let powered = state.get_value(&BlockStateProperties::POWERED);
        if attached || powered {
            Self::calculate_state(world, pos, state, true, false, -1, None);
        }
        if powered {
            Self::notify_neighbors(
                state.get_block(),
                world,
                pos,
                state.get_value(&BlockStateProperties::HORIZONTAL_FACING),
            );
        }
    }
}

impl BlockBehavior for TripWireHookBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let direction = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let support_pos = pos.relative(direction.opposite());
        direction.axis() != Axis::Y
            && world.is_face_sturdy(world.get_block_state(support_pos), support_pos, direction)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        for direction in context.get_nearest_looking_directions() {
            if direction.axis() == Axis::Y {
                continue;
            }
            let state = self
                .block
                .default_state()
                .set_value(
                    &BlockStateProperties::HORIZONTAL_FACING,
                    direction.opposite(),
                )
                .set_value(&BlockStateProperties::POWERED, false)
                .set_value(&BlockStateProperties::ATTACHED, false);
            if self.can_survive(state, context.world.as_ref(), context.place_pos()) {
                return Some(state);
            }
        }
        None
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
        if direction.opposite() == state.get_value(&BlockStateProperties::HORIZONTAL_FACING)
            && !self.can_survive(state, world, pos)
        {
            vanilla_blocks::AIR.default_state()
        } else {
            state
        }
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source: &PlacementSource<'_>,
    ) {
        Self::calculate_state(world, pos, state, false, false, -1, None);
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        Self::calculate_state(world, pos, state, false, true, -1, None);
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        if !moved_by_piston {
            Self::on_removed(state, world, pos);
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
        if state.get_value(&BlockStateProperties::POWERED) {
            15
        } else {
            0
        }
    }

    fn get_direct_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        direction: Direction,
        _context: SignalQueryContext,
    ) -> i32 {
        if state.get_value(&BlockStateProperties::POWERED)
            && state.get_value(&BlockStateProperties::HORIZONTAL_FACING) == direction
        {
            15
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn line_attachment_power_and_disarming_match_vanilla() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("tripwire_line");
        let left = BlockPos::new(5, 64, 8);
        let right = BlockPos::new(9, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(left));
        assert!(world.set_block(
            left.west(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            right.east(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let left_state = vanilla_blocks::TRIPWIRE_HOOK
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::East);
        let right_state = vanilla_blocks::TRIPWIRE_HOOK
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::West);
        assert!(world.set_block(left, left_state, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(right, right_state, UpdateFlags::UPDATE_NONE));
        for x in 6..=8 {
            assert!(world.set_block(
                BlockPos::new(x, 64, 8),
                vanilla_blocks::TRIPWIRE.default_state(),
                UpdateFlags::UPDATE_NONE,
            ));
        }

        TripWireHookBlock::calculate_state(&world, left, left_state, false, false, -1, None);
        assert!(
            world
                .get_block_state(left)
                .get_value(&BlockStateProperties::ATTACHED)
        );
        assert!(
            world
                .get_block_state(right)
                .get_value(&BlockStateProperties::ATTACHED)
        );

        let powered_wire = world
            .get_block_state(left.relative_n(Direction::East, 2))
            .set_value(&BlockStateProperties::POWERED, true);
        TripWireHookBlock::calculate_state(
            &world,
            left,
            world.get_block_state(left),
            false,
            true,
            2,
            Some(powered_wire),
        );
        assert!(
            world
                .get_block_state(left)
                .get_value(&BlockStateProperties::POWERED)
        );
        assert!(
            world
                .get_block_state(right)
                .get_value(&BlockStateProperties::POWERED)
        );

        TripWireHookBlock::calculate_state(
            &world,
            left,
            world.get_block_state(left),
            false,
            true,
            2,
            Some(powered_wire.set_value(&BlockStateProperties::DISARMED, true)),
        );
        let disarmed_hook = world.get_block_state(left);
        assert!(!disarmed_hook.get_value(&BlockStateProperties::ATTACHED));
        assert!(!disarmed_hook.get_value(&BlockStateProperties::POWERED));
    }
}
