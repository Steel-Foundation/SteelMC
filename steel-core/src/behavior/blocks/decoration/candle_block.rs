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
    items::item::BlockHitResult,
    vanilla_blocks,
};
use steel_utils::{
    BlockPos,
    types::{self, UpdateFlags},
};

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, InteractionResult, InventoryAccess},
    player,
    world::{LevelReader, ScheduledTickAccess, World},
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
        world
            .get_block_state(pos.below())
            .is_face_sturdy_for(Direction::Up, SupportType::Center)
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
}
