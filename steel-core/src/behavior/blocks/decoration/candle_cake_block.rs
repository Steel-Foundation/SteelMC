use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, BoolProperty},
    },
    item_stack::ItemStack,
    items::item::BlockHitResult,
    vanilla_blocks, vanilla_items,
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
    entity::projectile::Projectile,
    player::Player,
    world::{ClipHitResult, Explosion, LevelReader, ScheduledTickAccess, World},
};

/// Behavior for Candle Cakes
/// TODO: Add animation ticks.
#[block_behavior]
pub struct CandleCakeBlock {
    block: BlockRef,
}

const LIT: &BoolProperty = &BlockStateProperties::LIT;

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
            && state.get_value(LIT)
        {
            CandleBlock::extinguish(state, world, pos, Some(player));
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

    fn on_explosion_hit(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        explosion: &dyn Explosion,
        on_hit: &mut dyn FnMut(ItemStack, BlockPos),
    ) {
        if explosion.can_trigger_blocks() && state.get_value(LIT) {
            CandleBlock::extinguish(state, world, pos, None);
        }
        self.default_on_explosion_hit(state, world, pos, explosion, on_hit);
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
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        CakeBlock::analog_output_signal(0)
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }
}
