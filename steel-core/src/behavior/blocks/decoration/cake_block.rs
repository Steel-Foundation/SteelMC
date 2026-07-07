use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt, properties::BlockStateProperties},
    items::item::BlockHitResult,
    sound_events, vanilla_blocks,
    vanilla_item_tags::ItemTag,
};
use steel_utils::{
    BlockPos, BlockStateId, Direction,
    types::{InteractionHand, UpdateFlags},
};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, InteractionResult, InventoryAccess, candle_cakes,
    },
    entity::Entity,
    player::Player,
    world::{LevelReader, ScheduledTickAccess, World},
};

/// Behavior for Cakes
/// TODO:
/// - [ ] animation ticks
/// - [ ] onProjectile
/// - [ ] onExplosion
#[block_behavior]
pub struct CakeBlock {
    block: BlockRef,
}

impl CakeBlock {
    /// Cakes a new Cake Block Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Eats a slice of the cake for the player and updates the block
    pub fn eat(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        player: &Player,
    ) -> InteractionResult {
        if player.can_eat(false) {
            let mut food_data = player.food_data.lock();
            food_data.eat(2, 0.1);
            let bites = state.get_value(&BlockStateProperties::BITES);
            let new_state = if bites < 6 {
                state.set_value(&BlockStateProperties::BITES, bites + 1)
            } else {
                vanilla_blocks::AIR.default_state()
            };
            world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
            return InteractionResult::Success;
        }
        InteractionResult::Pass
    }

    /// Analog Output Signal for the Amount of Bites
    #[must_use]
    pub const fn analog_output_signal(bites: i32) -> i32 {
        (7 - bites) * 2
    }
}

impl BlockBehavior for CakeBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if context
            .world
            .get_block_state(context.place_pos.below())
            .is_solid()
        {
            Some(self.block.default_state())
        } else {
            None
        }
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        world.get_block_state(pos.below()).is_solid()
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
        if direction == Direction::Down && !self.can_survive(state, world, pos) {
            vanilla_blocks::AIR.default_state()
        } else {
            state
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
        if Self::eat(world, pos, state, player).consumes_action() {
            return InteractionResult::Success;
        }

        InteractionResult::Pass
    }

    fn use_item_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hand: InteractionHand,
        _hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        if state.get_value(&BlockStateProperties::BITES) == 0 {
            let candle_cake = inv.with_item(|item_stack| {
                let item = item_stack.item();
                if !item.has_tag(&ItemTag::CANDLES) {
                    return None;
                }
                let candle_cake = candle_cakes::candle_to_candle_cake(item)?;
                if !player.has_infinite_materials() {
                    item_stack.shrink(1);
                }
                Some(candle_cake)
            });
            let Some(candle_cake) = candle_cake else {
                return InteractionResult::TryEmptyHandInteraction;
            };
            world.play_block_sound(
                &sound_events::BLOCK_CAKE_ADD_CANDLE,
                pos,
                1.0,
                1.0,
                Some(player.id()),
            );
            world.set_block(pos, candle_cake.default_state(), UpdateFlags::UPDATE_ALL);
            return InteractionResult::Success;
        }
        InteractionResult::TryEmptyHandInteraction
    }

    fn get_analog_output_signal(
        &self,
        state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
    ) -> i32 {
        Self::analog_output_signal(i32::from(state.get_value(&BlockStateProperties::BITES)))
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }
}
