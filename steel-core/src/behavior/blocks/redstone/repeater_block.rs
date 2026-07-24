//! Vanilla redstone repeater behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use super::diode_block::DiodeBlock;
use crate::behavior::{
    BlockBehavior, BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess,
    PlacementSource,
};
use crate::player::Player;
use crate::world::{LevelReader, ScheduledTickAccess, SignalQueryContext, World};

/// Vanilla `RepeaterBlock`, including side locking and scheduled pulse behavior.
#[block_behavior]
pub struct RepeaterBlock {
    diode: DiodeBlock,
}

impl RepeaterBlock {
    /// Creates a repeater behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            diode: DiodeBlock::new(block),
        }
    }

    fn delay(state: BlockStateId) -> i32 {
        i32::from(state.get_value(&BlockStateProperties::DELAY)) * 2
    }

    fn is_locked_at(level: &dyn LevelReader, pos: BlockPos, state: BlockStateId) -> bool {
        DiodeBlock::get_alternate_signal(level, pos, state, true) > 0
    }

    fn should_turn_on(level: &dyn LevelReader, pos: BlockPos, state: BlockStateId) -> bool {
        DiodeBlock::get_input_signal(level, pos, state) > 0
    }

    fn check_tick_on_neighbor(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.diode.check_tick_on_neighbor(
            world,
            pos,
            state,
            Self::is_locked_at(world.as_ref(), pos, state),
            Self::should_turn_on(world.as_ref(), pos, state),
            Self::delay(state),
        );
    }
}

impl BlockBehavior for RepeaterBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        DiodeBlock::can_survive(world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.diode.state_for_placement(context);
        Some(state.set_value(
            &BlockStateProperties::LOCKED,
            Self::is_locked_at(context.world.as_ref(), context.place_pos(), state),
        ))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction == Direction::Down
            && !DiodeBlock::can_survive_on(world, neighbor_pos, neighbor_state)
        {
            return REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        }

        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        if direction.get_axis() == facing.get_axis() {
            state
        } else {
            state.set_value(
                &BlockStateProperties::LOCKED,
                Self::is_locked_at(world, pos, state),
            )
        }
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        if !player.abilities.lock().may_build {
            return InteractionResult::Pass;
        }

        let delay = state.get_value(&BlockStateProperties::DELAY);
        let next_delay = if delay == 4 { 1 } else { delay + 1 };
        world.set_block(
            pos,
            state.set_value(&BlockStateProperties::DELAY, next_delay),
            UpdateFlags::UPDATE_ALL,
        );
        InteractionResult::Success
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        self.diode.handle_neighbor_changed(state, world, pos, || {
            self.check_tick_on_neighbor(state, world, pos);
        });
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.diode.tick(
            state,
            world,
            pos,
            Self::is_locked_at(world.as_ref(), pos, state),
            Self::should_turn_on(world.as_ref(), pos, state),
            Self::delay(state),
        );
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source: &PlacementSource<'_>,
    ) {
        self.diode
            .set_placed_by(world, pos, Self::should_turn_on(world.as_ref(), pos, state));
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        self.diode.on_place(state, world, pos);
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        self.diode
            .affect_neighbors_after_removal(state, world, pos, moved_by_piston);
    }

    fn is_signal_source(&self, _state: BlockStateId, _context: SignalQueryContext) -> bool {
        true
    }

    fn is_diode(&self) -> bool {
        true
    }

    fn get_own_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        DiodeBlock::own_signal(state, 15)
    }

    fn get_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        direction: Direction,
        _context: SignalQueryContext,
    ) -> i32 {
        DiodeBlock::signal(state, direction, 15)
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

    // `animateTick` emits client-local dust particles only.
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;
    use crate::world::tick_scheduler::TickPriority;

    fn repeater() -> RepeaterBlock {
        init_test_registry();
        init_behaviors();
        RepeaterBlock::new(&vanilla_blocks::REPEATER)
    }

    fn repeater_state(facing: Direction, powered: bool) -> BlockStateId {
        vanilla_blocks::REPEATER
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
            .set_value(&BlockStateProperties::POWERED, powered)
    }

    #[test]
    fn input_reads_wire_power_in_front() {
        let _repeater = repeater();
        let pos = BlockPos::new(0, 64, 0);
        let state = repeater_state(Direction::East, false);
        let wire = vanilla_blocks::REDSTONE_WIRE
            .default_state()
            .set_value(&BlockStateProperties::POWER, 7);
        let level = TestLevel::default().with_block(pos.east(), wire);

        assert_eq!(DiodeBlock::get_input_signal(&level, pos, state), 7);
    }

    #[test]
    fn side_lock_accepts_only_another_diode() {
        let _repeater = repeater();
        let pos = BlockPos::new(0, 64, 0);
        let state = repeater_state(Direction::North, false);
        let side_repeater = repeater_state(Direction::East, true);
        let level = TestLevel::default().with_block(pos.east(), side_repeater);
        assert!(RepeaterBlock::is_locked_at(&level, pos, state));

        let redstone_block_level = TestLevel::default()
            .with_block(pos.east(), vanilla_blocks::REDSTONE_BLOCK.default_state());
        assert!(!RepeaterBlock::is_locked_at(
            &redstone_block_level,
            pos,
            state
        ));
    }

    #[test]
    fn powered_output_is_directional() {
        let repeater = repeater();
        let state = repeater_state(Direction::West, true);
        let level = TestLevel::default();
        let pos = BlockPos::new(0, 64, 0);

        assert_eq!(
            repeater.get_signal(
                state,
                &level,
                pos,
                Direction::West,
                SignalQueryContext::DEFAULT,
            ),
            15
        );
        assert_eq!(
            repeater.get_signal(
                state,
                &level,
                pos,
                Direction::East,
                SignalQueryContext::DEFAULT,
            ),
            0
        );
    }

    #[test]
    fn support_requires_vanilla_rigid_face() {
        let repeater = repeater();
        let pos = BlockPos::new(0, 64, 0);
        let stone =
            TestLevel::default().with_block(pos.below(), vanilla_blocks::STONE.default_state());
        let air = TestLevel::default();

        assert!(repeater.can_survive(vanilla_blocks::REPEATER.default_state(), &stone, pos));
        assert!(!repeater.can_survive(vanilla_blocks::REPEATER.default_state(), &air, pos));
    }

    #[test]
    fn neighbor_tick_priority_matches_vanilla_diode_ordering() {
        let _repeater = repeater();
        let pos = BlockPos::new(0, 64, 0);
        let off = repeater_state(Direction::North, false);
        let on = repeater_state(Direction::North, true);
        let plain_level = TestLevel::default();

        assert_eq!(
            DiodeBlock::tick_priority(&plain_level, pos, off, false),
            TickPriority::High
        );
        assert_eq!(
            DiodeBlock::tick_priority(&plain_level, pos, on, true),
            TickPriority::VeryHigh
        );

        let crossing_diode = repeater_state(Direction::East, false);
        let prioritized_level = TestLevel::default().with_block(pos.south(), crossing_diode);
        assert_eq!(
            DiodeBlock::tick_priority(&prioritized_level, pos, off, false),
            TickPriority::ExtremelyHigh
        );
    }
}
