use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    item_stack::ItemStack,
    items::item::BlockHitResult,
    sound_events, vanilla_blocks, vanilla_game_events, vanilla_items,
};
use steel_utils::{
    Direction,
    types::{InteractionHand, UpdateFlags},
};

use crate::{
    behavior::{InteractionResult, InventoryAccess, ItemBehavior},
    entity::Entity,
    player::Player,
    world::{World, game_event_context::GameEventContext},
};

/// Behavior for Hoes
#[item_behavior]
pub struct HoeItem;

impl HoeItem {
    fn get_tilled_variant(block: BlockRef) -> Option<BlockRef> {
        match block {
            _ if block == &vanilla_blocks::GRASS_BLOCK
                || block == &vanilla_blocks::DIRT_PATH
                || block == &vanilla_blocks::DIRT =>
            {
                Some(&vanilla_blocks::FARMLAND)
            }
            _ if block == &vanilla_blocks::COARSE_DIRT => Some(&vanilla_blocks::DIRT),
            _ if block == &vanilla_blocks::ROOTED_DIRT => Some(&vanilla_blocks::DIRT),
            _ => None,
        }
    }
}

impl ItemBehavior for HoeItem {
    fn use_on(
        &self,
        player: &Player,
        _hand: InteractionHand,
        hit_result: BlockHitResult,
        world: &Arc<World>,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let state = world.get_block_state(hit_result.block_pos);
        let Some(tilled_variant) = Self::get_tilled_variant(state.get_block()) else {
            return InteractionResult::Pass;
        };

        if (hit_result.direction == Direction::Down
            || !world.get_block_state(hit_result.block_pos.above()).is_air())
            && state.get_block() != &vanilla_blocks::ROOTED_DIRT
        {
            return InteractionResult::Pass;
        }

        let new_state = tilled_variant.default_state();
        world.set_block(
            hit_result.block_pos,
            new_state,
            UpdateFlags::UPDATE_ALL_IMMEDIATE,
        );
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            hit_result.block_pos,
            &GameEventContext::new(Some(player), Some(new_state)),
        );

        if state.get_block() == &vanilla_blocks::ROOTED_DIRT {
            world.pop_resource_from_face(
                hit_result.block_pos,
                hit_result.direction,
                ItemStack::new(&vanilla_items::ITEMS.hanging_roots),
            );
        }

        world.play_block_sound(
            &sound_events::ITEM_HOE_TILL,
            hit_result.block_pos,
            1.0,
            1.0,
            Some(player.id()),
        );

        let has_infinite_materials = player.has_infinite_materials();
        inv.with_item(|item| item.hurt_and_break(1, has_infinite_materials));

        InteractionResult::Success
    }
}
