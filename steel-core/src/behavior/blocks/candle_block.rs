use std::ptr;

use steel_registry::{
    REGISTRY,
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, BoolProperty, IntProperty},
        shapes::SupportType,
    },
    entity_data::Direction,
    item_stack::ItemStack,
    items::item::BlockHitResult,
    vanilla_blocks,
};
use steel_utils::{
    BlockPos, Identifier,
    types::{self, UpdateFlags},
};

use crate::{
    behavior::{BlockBehaviour, BlockPlaceContext, InteractionResult},
    player,
    world::World,
};

const CANDLES_PROPERTY: IntProperty = BlockStateProperties::CANDLES;
const LIT_PROPERTY: BoolProperty = BlockStateProperties::LIT;
const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;
const MAX_CANDLES: u8 = 4;

/// Behaviour for all Candle type blocks
pub struct CandleBlock {
    block: BlockRef,
}

impl CandleBlock {
    /// Creates a new candle block behaviour for the given block
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Checks if the Candle block can survive at the given position
    pub fn can_survive(world: &World, pos: BlockPos) -> bool {
        world
            .get_block_state(&pos.below())
            .is_face_sturdy_for(Direction::Up, SupportType::Center)
    }
}

impl BlockBehaviour for CandleBlock {
    fn get_state_for_placement(
        &self,
        context: &BlockPlaceContext<'_>,
    ) -> Option<steel_utils::BlockStateId> {
        if Self::can_survive(context.world, context.relative_pos) {
            let default_state = self.block.default_state();
            if ptr::eq(
                context
                    .world
                    .get_block_state(&context.relative_pos)
                    .get_block(),
                vanilla_blocks::WATER,
            ) {
                // FIXME: is_water_source()
                return Some(default_state.set_value(&WATERLOGGED, true));
            }
            return Some(default_state);
        }
        None
    }

    fn update_shape(
        &self,
        state: steel_utils::BlockStateId,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: steel_utils::BlockStateId,
    ) -> steel_utils::BlockStateId {
        if !Self::can_survive(world, pos) {
            return REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
        }
        state
    }

    fn use_item_on(
        &self,
        item_stack: &ItemStack,
        state: steel_utils::BlockStateId,
        world: &World,
        pos: BlockPos,
        _player: &player::Player,
        _hand: types::InteractionHand,
        _hit_result: &BlockHitResult,
    ) -> InteractionResult {
        if item_stack.is_empty() {
            if !state.get_value(&LIT_PROPERTY) {
                return InteractionResult::Pass;
            }
            let new_state = state.set_value(&LIT_PROPERTY, false);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);
            return InteractionResult::Success;
        }

        if REGISTRY.items.is_in_tag(
            item_stack.item,
            &Identifier::vanilla_static("creeper_igniters"),
        ) {
            if state.get_value(&LIT_PROPERTY) || state.get_value(&WATERLOGGED) {
                return InteractionResult::Pass;
            }
            let new_state = state.set_value(&LIT_PROPERTY, true);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);
            return InteractionResult::Success;
        }

        if self
            .get_clone_item_stack(self.block, state, false)
            .is_some_and(|it| it.is(item_stack.item))
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
