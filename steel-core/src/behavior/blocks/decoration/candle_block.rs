use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    REGISTRY,
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, BoolProperty, IntProperty},
        shapes::SupportType,
    },
    entity_data::Direction,
    fluid::FluidState,
    items::item::BlockHitResult,
    sound_events, vanilla_blocks, vanilla_fluids, vanilla_game_events,
};
use steel_utils::{
    BlockPos,
    types::{self, UpdateFlags},
};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, InteractionResult, InventoryAccess,
        block::schedule_placed_liquid_tick,
    },
    player,
    world::{
        LevelAccessor, LevelReader, ScheduledTickAccess, World,
        game_event_context::GameEventContext,
    },
};

const CANDLES_PROPERTY: IntProperty = BlockStateProperties::CANDLES;
const LIT_PROPERTY: BoolProperty = BlockStateProperties::LIT;
const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;
const MAX_CANDLES: u8 = 4;

/// Behavior for all Candle type blocks
#[block_behavior]
pub struct CandleBlock {
    block: BlockRef,
}

impl CandleBlock {
    /// Creates a new candle block behavior for the given block
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for CandleBlock {
    /// Checks if the candle block can survive at the given position.
    fn can_survive(
        &self,
        _state: steel_utils::BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let below_pos = pos.below();
        world.get_block_state(below_pos).is_face_sturdy_for_at(
            below_pos,
            Direction::Up,
            SupportType::Center,
        )
    }

    fn get_state_for_placement(
        &self,
        context: &BlockPlaceContext<'_>,
    ) -> Option<steel_utils::BlockStateId> {
        let default_state = self.block.default_state();
        if self.can_survive(default_state, context.world, context.relative_pos) {
            return Some(default_state.set_value(&WATERLOGGED, context.is_water_source()));
        }
        None
    }

    fn update_shape(
        &self,
        state: steel_utils::BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: steel_utils::BlockStateId,
    ) -> steel_utils::BlockStateId {
        if state.get_value(&WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        if !self.can_survive(state, world, pos) {
            return REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        }
        state
    }

    fn use_item_on(
        &self,
        state: steel_utils::BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _player: &player::Player,
        _hand: types::InteractionHand,
        _hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let item_is_empty = inv.with_item(|item_stack| item_stack.is_empty());
        if item_is_empty {
            if !state.get_value(&LIT_PROPERTY) {
                return InteractionResult::Pass;
            }
            let new_state = state.set_value(&LIT_PROPERTY, false);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);
            return InteractionResult::Success;
        }

        if self
            .get_clone_item_stack(self.block, state, false)
            .is_some_and(|it| inv.with_item(|item_stack| it.is(item_stack.item)))
        {
            let candles_amount = state.get_value(&CANDLES_PROPERTY);
            if candles_amount < MAX_CANDLES {
                let new_state = state.set_value(&CANDLES_PROPERTY, candles_amount + 1);
                world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);
                return InteractionResult::Success;
            }
        }

        InteractionResult::TryEmptyHandInteraction
    }

    fn place_liquid(
        &self,
        level: &dyn LevelAccessor,
        pos: BlockPos,
        state: steel_utils::BlockStateId,
        fluid_state: FluidState,
    ) -> bool {
        if state.try_get_value(&WATERLOGGED) != Some(false)
            || fluid_state.fluid_id != &vanilla_fluids::WATER
        {
            return false;
        }

        let waterlogged = state.set_value(&WATERLOGGED, true);
        if state.get_value(&LIT_PROPERTY) {
            let extinguished = waterlogged.set_value(&LIT_PROPERTY, false);
            level.set_block_state(pos, extinguished, UpdateFlags::UPDATE_ALL_IMMEDIATE);
            level.play_block_sound(&sound_events::BLOCK_CANDLE_EXTINGUISH, pos, 1.0, 1.0, None);
            level.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                pos,
                &GameEventContext::new(None, Some(extinguished)),
            );
        } else {
            level.set_block_state(pos, waterlogged, UpdateFlags::UPDATE_ALL);
        }

        schedule_placed_liquid_tick(level, pos, fluid_state);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestLevel;
    use steel_registry::test_support::init_test_registry;

    fn supporting_level() -> TestLevel {
        TestLevel::default().with_block(
            BlockPos::ZERO.below(),
            vanilla_blocks::STONE.default_state(),
        )
    }

    #[test]
    fn waterlogged_candle_update_shape_schedules_water_tick() {
        init_test_registry();

        let candle = CandleBlock::new(&vanilla_blocks::CANDLE);
        let state = vanilla_blocks::CANDLE
            .default_state()
            .set_value(&WATERLOGGED, true);
        let level = supporting_level();

        assert_eq!(
            candle.update_shape(
                state,
                &level,
                BlockPos::ZERO,
                Direction::North,
                Direction::North.relative(BlockPos::ZERO),
                vanilla_blocks::AIR.default_state(),
            ),
            state
        );
        assert_eq!(
            level
                .scheduled_fluid_ticks
                .borrow()
                .iter()
                .map(|tick| tick.fluid)
                .collect::<Vec<_>>(),
            vec![&vanilla_fluids::WATER]
        );
    }

    #[test]
    fn water_placement_on_lit_candle_emits_block_change_event() {
        init_test_registry();

        let candle = CandleBlock::new(&vanilla_blocks::CANDLE);
        let state = vanilla_blocks::CANDLE
            .default_state()
            .set_value(&WATERLOGGED, false)
            .set_value(&LIT_PROPERTY, true);
        let level = supporting_level();

        assert!(candle.place_liquid(
            &level,
            BlockPos::ZERO,
            state,
            FluidState::source(&vanilla_fluids::WATER),
        ));

        assert_eq!(
            level
                .block_sounds
                .borrow()
                .iter()
                .map(|sound| sound.sound)
                .collect::<Vec<_>>(),
            vec![&sound_events::BLOCK_CANDLE_EXTINGUISH]
        );
        assert_eq!(
            level
                .game_events
                .borrow()
                .iter()
                .map(|event| event.event)
                .collect::<Vec<_>>(),
            vec![&vanilla_game_events::BLOCK_CHANGE]
        );
        assert!(
            level
                .last_placed_state()
                .expect("candle should be waterlogged")
                .get_value(&WATERLOGGED)
        );
    }
}
