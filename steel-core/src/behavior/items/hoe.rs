use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    sound_events, vanilla_blocks,
};
use steel_utils::{Direction, types::UpdateFlags};

use crate::behavior::{InteractionResult, ItemBehavior, UseOnContext};

/// Behavior for Hoes
pub struct HoeBehavior;

impl HoeBehavior {
    fn get_tilled_variant(block: BlockRef) -> Option<BlockRef> {
        match block {
            b if b == vanilla_blocks::GRASS_BLOCK
                || b == vanilla_blocks::DIRT_PATH
                || b == vanilla_blocks::DIRT =>
            {
                Some(vanilla_blocks::FARMLAND)
            }
            b if b == vanilla_blocks::COARSE_DIRT => Some(vanilla_blocks::DIRT),
            b if b == vanilla_blocks::ROOTED_DIRT => Some(vanilla_blocks::DIRT),
            _ => None,
        }
    }
}

impl ItemBehavior for HoeBehavior {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let state = context.world.get_block_state(&context.hit_result.block_pos);
        let Some(tilled_variant) = Self::get_tilled_variant(state.get_block()) else {
            return InteractionResult::Pass;
        };

        if (context.hit_result.direction == Direction::Down
            || !context
                .world
                .get_block_state(&context.hit_result.block_pos.above())
                .is_air())
            && state.get_block() != vanilla_blocks::ROOTED_DIRT
        {
            return InteractionResult::Pass;
        }

        if state.get_block() == vanilla_blocks::ROOTED_DIRT {
            // TODO: Spawn Item
        }

        context.world.set_block(
            context.hit_result.block_pos,
            tilled_variant.default_state(),
            UpdateFlags::UPDATE_ALL_IMMEDIATE,
        );

        context.world.play_block_sound(
            sound_events::ITEM_HOE_TILL,
            context.hit_result.block_pos,
            1.0,
            1.0,
            Some(context.player.id),
        );

        InteractionResult::Success
    }
}
