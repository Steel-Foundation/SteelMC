use steel_macros::item_behavior;
use steel_registry::{REGISTRY, blocks::block_state_ext::BlockStateExt, level_events};
use steel_utils::types::UpdateFlags;

use crate::{
    behavior::{
        InteractionResult, ItemBehavior, UseOnContext, waxables::get_waxed_from_normal_variant,
    },
    block_entity::{BlockEntity, entities::SignBlockEntity},
};

/// Behavior for the honeycomb item. Waxes copper blocks and signs.
#[item_behavior(class = "HoneycombItem")]
pub struct HoneycombBehavior;

impl ItemBehavior for HoneycombBehavior {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let pos = context.hit_result.block_pos;

        // Try block waxing first
        let old_block_state = context.world.get_block_state(pos);
        if let Some(waxed_block) = get_waxed_from_normal_variant(old_block_state.get_block()) {
            context.item_stack.shrink(1);
            // TODO: trigger CriteriaTriggers.ITEM_USED_ON_BLOCK advancement
            context.world.set_block(
                pos,
                REGISTRY
                    .blocks
                    .copy_matching_properties(old_block_state, waxed_block),
                UpdateFlags::UPDATE_ALL,
            );
            // TODO: dispatch GameEvent::BLOCK_CHANGE
            context.world.level_event(
                level_events::PARTICLES_AND_SOUND_WAX_ON,
                pos,
                0,
                Some(context.player.id),
            );
            return InteractionResult::Success;
        }

        // Fall through to sign waxing
        let Some(block_entity) = context.world.get_block_entity(pos) else {
            return InteractionResult::Pass;
        };

        let mut guard = block_entity.lock();
        let Some(sign) = guard.as_any_mut().downcast_mut::<SignBlockEntity>() else {
            return InteractionResult::Pass;
        };

        if sign.is_waxed {
            return InteractionResult::Pass;
        }

        sign.is_waxed = true;
        sign.set_changed();
        context.item_stack.shrink(1);
        context.world.level_event(
            level_events::PARTICLES_AND_SOUND_WAX_ON,
            pos,
            0,
            Some(context.player.id),
        );
        InteractionResult::Success
    }
}
