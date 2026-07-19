use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt, properties::BlockStateProperties},
    item_stack::ItemStack,
    items::item::BlockHitResult,
    sound_events, vanilla_blocks, vanilla_items,
};
use steel_utils::{
    BlockPos, BlockStateId, Direction,
    types::{InteractionHand, UpdateFlags},
};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, InteractionResult, InventoryAccess,
        blocks::{CakeBlock, CandleBlock},
    },
    entity::{Entity, projectile::Projectile},
    player::Player,
    world::{ClipHitResult, LevelReader, ScheduledTickAccess, World},
};

/// Behavior for Candle Cakes
/// TODO:
/// - [ ] animation ticks
/// - [ ] onExplosion
#[block_behavior]
pub struct CandleCakeBlock {
    block: BlockRef,
}

impl CandleCakeBlock {
    /// Creates a new Candle Cake Block Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for CandleCakeBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if context
            .world
            .get_block_state(context.place_pos().below())
            .is_solid()
        {
            Some(self.block.default_state())
        } else {
            None
        }
    }

    fn use_item_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hand: InteractionHand,
        hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let (is_fire_charge, is_flint_and_steel, is_empty) = inv.with_item(|item_stack| {
            (
                item_stack.is(&vanilla_items::FIRE_CHARGE),
                item_stack.is(&vanilla_items::FLINT_AND_STEEL),
                item_stack.is_empty(),
            )
        });
        if is_fire_charge || is_flint_and_steel {
            return InteractionResult::Pass; // lighting of candles and candle cakes is handled by the flint and steel/fire charge implementation
        } else if (hit_result.location.y - f64::from(hit_result.block_pos.y())) > 0.5
            && is_empty
            && state.get_value(&BlockStateProperties::LIT)
        {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::LIT, false),
                UpdateFlags::UPDATE_ALL,
            );
            // TODO: particles!
            world.play_block_sound(
                &sound_events::BLOCK_CANDLE_EXTINGUISH,
                pos,
                1.0,
                1.0,
                Some(player.id()),
            );
            return InteractionResult::Success;
        }
        InteractionResult::TryEmptyHandInteraction
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
        let result = CakeBlock::eat(world, pos, vanilla_blocks::CAKE.default_state(), player);
        if result.consumes_action() {
            world.drop_resources(state, pos);
        }
        result
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        world.get_block_state(pos.below()).is_solid()
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: steel_utils::Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction == Direction::Down && !self.can_survive(state, world, pos) {
            vanilla_blocks::AIR.default_state()
        } else {
            state
        }
    }

    fn on_projectile_hit(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        hit: &ClipHitResult,
        projectile: &dyn Projectile,
    ) {
        let Some(lit_state) = CandleBlock::projectile_lit_state(state, projectile.is_on_fire())
        else {
            return;
        };
        world.set_block(hit.block_pos, lit_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(&vanilla_items::CAKE))
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
    ) -> i32 {
        CakeBlock::analog_output_signal(0)
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }
}
