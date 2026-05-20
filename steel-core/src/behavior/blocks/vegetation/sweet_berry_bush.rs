use std::sync::Arc;

use rand::RngExt;
use steel_macros::block_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt, properties::BlockStateProperties},
    item_stack::ItemStack,
    items::item::BlockHitResult,
    loot_table::LootContext,
    sound_events, vanilla_entities, vanilla_items,
    vanilla_loot_tables::{self},
};
use steel_utils::{
    BlockPos, BlockStateId,
    types::{InteractionHand, UpdateFlags},
};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, InteractionResult,
        blocks::vegetation::{
            Vegetation,
            bonemealable::Bonemealable,
            vegetation_block::{vegetation_can_survive, vegetation_update_shape},
        },
    },
    entity::Entity,
    player::Player,
    world::{LevelReader, ScheduledTickAccess, World},
};

/// Behavior for Sweet Berry Bushes
#[block_behavior]
pub struct SweetBerryBushBlock {
    block: BlockRef,
}

impl SweetBerryBushBlock {
    /// Creates a new Sweet Berry Bush Block Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SweetBerryBushBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if self.may_place_on(
            context.world.get_block_state(context.relative_pos.below()),
            context.world,
            context.relative_pos.below(),
        ) {
            Some(
                self.block
                    .default_state()
                    .set_value(&BlockStateProperties::AGE_3, 0),
            )
        } else {
            None
        }
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: steel_utils::Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        vegetation_update_shape(self, state, world, pos)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        vegetation_can_survive(self, state, world, pos)
    }

    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        state.get_value(&BlockStateProperties::AGE_3) < 3
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let age = state.get_value(&BlockStateProperties::AGE_3);
        if age >= 3 || rand::random_range(0..5) != 0 || world.raw_brightness(pos.above(), 0) < 9 {
            return;
        }
        world.set_block(
            pos,
            state.set_value(&BlockStateProperties::AGE_3, age + 1),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }

    fn entity_inside(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        entity: &dyn Entity,
    ) {
        if entity.entity_type() == &vanilla_entities::FOX
            || entity.entity_type() == &vanilla_entities::BEE
        {
            return;
        }
        let Some(_living) = entity.base() else {
            return;
        };

        // TODO: make stuck in block
    }

    fn use_item_on(
        &self,
        item_stack: &ItemStack,
        state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        _player: &Player,
        _hand: InteractionHand,
        _hit_result: &BlockHitResult,
    ) -> InteractionResult {
        let age = state.get_value(&BlockStateProperties::AGE_3);
        if age != 3 && item_stack.is(&vanilla_items::ITEMS.bone_meal) {
            InteractionResult::Pass
        } else {
            InteractionResult::TryEmptyHandInteraction
        }
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
    ) -> InteractionResult {
        let age = state.get_value(&BlockStateProperties::AGE_3);
        if age <= 1 {
            return InteractionResult::Pass;
        }
        let mut rng = rand::rng();
        let mut ctx = LootContext::new(&mut rng).with_block_state(state);

        let items = vanilla_loot_tables::HARVEST_SWEET_BERRY_BUSH.get_random_items(&mut ctx);
        for item in items {
            world.drop_item_stack(pos, item);
        }

        world.play_block_sound(
            sound_events::BLOCK_SWEET_BERRY_BUSH_PICK_BERRIES,
            pos,
            1.0,
            0.8 + rng.random::<f32>() * 0.4,
            Some(player.id),
        );

        let new_state = state.set_value(&BlockStateProperties::AGE_3, 1);
        world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);

        InteractionResult::Success
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(&vanilla_items::ITEMS.sweet_berries))
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for SweetBerryBushBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        state.get_value(&BlockStateProperties::AGE_3) < 3
            && world.get_block_state(pos.above()).is_air()
            && !world.is_outside_build_height(pos.above().y())
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn rand::Rng,
        pos: BlockPos,
    ) {
        let new_age = (state.get_value(&BlockStateProperties::AGE_3) + 1).min(3);
        world.set_block(
            pos,
            state.set_value(&BlockStateProperties::AGE_3, new_age),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }
}

impl Vegetation for SweetBerryBushBlock {}
