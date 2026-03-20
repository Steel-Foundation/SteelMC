use steel_macros::item_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    item_stack::ItemStack,
    sound_events, vanilla_blocks, vanilla_items,
};
use steel_utils::{Direction, types::UpdateFlags};

use crate::behavior::{InteractionResult, ItemBehavior, UseOnContext};

/// Behavior for Hoes
#[item_behavior]
pub struct HoeItem;

impl HoeItem {
    fn get_tilled_variant(block: BlockRef) -> Option<BlockRef> {
        match block {
            _ if block == vanilla_blocks::GRASS_BLOCK
                || block == vanilla_blocks::DIRT_PATH
                || block == vanilla_blocks::DIRT =>
            {
                Some(vanilla_blocks::FARMLAND)
            }
            _ if block == vanilla_blocks::COARSE_DIRT => Some(vanilla_blocks::DIRT),
            _ if block == vanilla_blocks::ROOTED_DIRT => Some(vanilla_blocks::DIRT),
            _ => None,
        }
    }
}

impl ItemBehavior for HoeItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let state = context.world.get_block_state(context.hit_result.block_pos);
        let Some(tilled_variant) = Self::get_tilled_variant(state.get_block()) else {
            return InteractionResult::Pass;
        };

        if (context.hit_result.direction == Direction::Down
            || !context
                .world
                .get_block_state(context.hit_result.block_pos.above())
                .is_air())
            && state.get_block() != vanilla_blocks::ROOTED_DIRT
        {
            return InteractionResult::Pass;
        }

        context.world.set_block(
            context.hit_result.block_pos,
            tilled_variant.default_state(),
            UpdateFlags::UPDATE_ALL_IMMEDIATE,
        );
        // TODO: Emit GameEvent::BLOCK_CHANGE

        if state.get_block() == vanilla_blocks::ROOTED_DIRT {
            context.world.pop_resource_from_face(
                context.hit_result.block_pos,
                context.hit_result.direction,
                ItemStack::new(&vanilla_items::ITEMS.hanging_roots),
            );
        }

        context.world.play_block_sound(
            sound_events::ITEM_HOE_TILL,
            context.hit_result.block_pos,
            1.0,
            1.0,
            Some(context.player.id),
        );

        let has_infinite_materials = context.player.has_infinite_materials();
        context.inv.item().hurt_and_break(1, has_infinite_materials);

        InteractionResult::Success
    }
}
