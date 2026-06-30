use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::fluid::FluidState;
use steel_registry::sound_events;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_fluid_tags::FluidTag;
use steel_registry::vanilla_fluids;
use steel_registry::vanilla_items;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{BlockStateBehaviorExt, block::BlockBehavior, block::PickupResult};
use crate::player::Player;
use crate::world::{LevelAccessor, LevelReader, ScheduledTickAccess, World};

/// Vanilla `BubbleColumnBlock` column propagation and fluid state.
#[block_behavior]
pub struct BubbleColumnBlock {
    block: BlockRef,
}

impl BubbleColumnBlock {
    /// Creates a bubble column block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    pub(super) fn update_column(
        bubble_column: BlockRef,
        level: &dyn LevelAccessor,
        occupy_at: BlockPos,
        below_state: BlockStateId,
    ) {
        let occupy_state = level.get_block_state(occupy_at);
        if !Self::can_occupy(bubble_column, occupy_state) {
            return;
        }

        let column_state = Self::column_state(bubble_column, below_state, occupy_state);
        level.set_block_state(occupy_at, column_state, UpdateFlags::UPDATE_CLIENTS);

        let mut pos = occupy_at.above();
        while Self::can_occupy(bubble_column, level.get_block_state(pos)) {
            if !level.set_block_state(pos, column_state, UpdateFlags::UPDATE_CLIENTS) {
                return;
            }
            pos = pos.above();
        }
    }

    pub(super) fn can_occupy(bubble_column: BlockRef, occupy_state: BlockStateId) -> bool {
        if occupy_state.get_block() == bubble_column {
            return true;
        }

        let fluid_state = occupy_state.get_fluid_state();
        fluid_state
            .fluid_id
            .has_tag(&FluidTag::BUBBLE_COLUMN_CAN_OCCUPY)
            && occupy_state.get_block() == &vanilla_blocks::WATER
            && fluid_state.is_source()
            && fluid_state.amount >= 8
    }

    fn column_state(
        bubble_column: BlockRef,
        below_state: BlockStateId,
        occupy_state: BlockStateId,
    ) -> BlockStateId {
        if below_state.get_block() == bubble_column {
            return below_state;
        }
        if below_state
            .get_block()
            .has_tag(&BlockTag::ENABLES_BUBBLE_COLUMN_PUSH_UP)
        {
            return bubble_column
                .default_state()
                .set_value(&BlockStateProperties::DRAG, false);
        }
        if below_state
            .get_block()
            .has_tag(&BlockTag::ENABLES_BUBBLE_COLUMN_DRAG_DOWN)
        {
            return bubble_column
                .default_state()
                .set_value(&BlockStateProperties::DRAG, true);
        }

        if occupy_state.get_block() == bubble_column {
            vanilla_blocks::WATER.default_state()
        } else {
            occupy_state
        }
    }
}

impl BlockBehavior for BubbleColumnBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below = world.get_block_state(pos.below());
        below.get_block() == self.block
            || below
                .get_block()
                .has_tag(&BlockTag::ENABLES_BUBBLE_COLUMN_PUSH_UP)
            || below
                .get_block()
                .has_tag(&BlockTag::ENABLES_BUBBLE_COLUMN_DRAG_DOWN)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
        let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);

        if !self.can_survive(state, world, pos)
            || direction == Direction::Down
            || (direction == Direction::Up
                && neighbor_state.get_block() != self.block
                && Self::can_occupy(self.block, neighbor_state))
        {
            let _ = world.schedule_block_tick_default(pos, self.block, 5);
        }

        state
    }

    fn tick(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        Self::update_column(self.block, world, pos, world.get_block_state(pos.below()));
    }

    fn get_fluid_state(&self, _state: BlockStateId) -> FluidState {
        FluidState::source(&vanilla_fluids::WATER)
    }

    fn pickup_block(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        _state: BlockStateId,
        _player: Option<&Player>,
    ) -> Option<PickupResult> {
        world.set_block(
            pos,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_ALL_IMMEDIATE,
        );
        Some(PickupResult {
            filled_bucket: &vanilla_items::ITEMS.water_bucket,
            sound: Some(&sound_events::ITEM_BUCKET_FILL),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;
    use steel_registry::test_support::init_test_registry;

    #[test]
    fn bubble_column_update_shape_schedules_water_and_column_tick() {
        init_test_registry();
        let behavior = BubbleColumnBlock::new(&vanilla_blocks::BUBBLE_COLUMN);
        let level = TestLevel::default();
        let state = vanilla_blocks::BUBBLE_COLUMN.default_state();

        let updated = behavior.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::SOUL_SAND.default_state(),
        );

        assert_eq!(updated, state);
        assert!(level.scheduled_water_tick());
        assert!(
            level
                .scheduled_block_ticks
                .borrow()
                .iter()
                .any(|tick| tick.block == &vanilla_blocks::BUBBLE_COLUMN && tick.delay == 5)
        );
    }

    #[test]
    fn bubble_column_update_column_uses_push_up_and_drag_down_blocks() {
        init_test_registry();
        init_behaviors();
        let level = TestLevel::default()
            .with_block(BlockPos::ZERO, vanilla_blocks::WATER.default_state())
            .with_block(
                BlockPos::ZERO.above(),
                vanilla_blocks::WATER.default_state(),
            );

        BubbleColumnBlock::update_column(
            &vanilla_blocks::BUBBLE_COLUMN,
            &level,
            BlockPos::ZERO,
            vanilla_blocks::SOUL_SAND.default_state(),
        );

        let placed = level.placed_blocks.borrow();
        assert_eq!(placed.len(), 2);
        assert!(placed.iter().all(|placed| {
            placed.state.get_block() == &vanilla_blocks::BUBBLE_COLUMN
                && !placed.state.get_value(&BlockStateProperties::DRAG)
        }));
    }
}
