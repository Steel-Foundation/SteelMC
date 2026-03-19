use steel_macros::item_behavior;
use steel_registry::{REGISTRY, blocks::block_state_ext::BlockStateExt};
use steel_utils::types::UpdateFlags;

use crate::{
    behavior::{
        InteractionResult, ItemBehavior, UseOnContext, waxables::get_waxed_from_normal_variant,
    },
    block_entity::{BlockEntity, entities::SignBlockEntity},
};

/// Behavior for the Honey Comb Item
#[item_behavior(class = "HoneycombItem")]
pub struct HoneycombBehavior;

impl ItemBehavior for HoneycombBehavior {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        if let Some(block_entity) = context.world.get_block_entity(context.hit_result.block_pos) {
            let mut guard = block_entity.lock();
            let Some(sign) = guard.as_any_mut().downcast_mut::<SignBlockEntity>() else {
                return InteractionResult::Pass;
            };

            if sign.is_waxed {
                return InteractionResult::Pass;
            }

            sign.is_waxed = true;
            sign.set_changed();
            return InteractionResult::Success;
        }

        let old_block_state = context.world.get_block_state(context.hit_result.block_pos);

        let Some(waxed_block) = get_waxed_from_normal_variant(old_block_state.get_block()) else {
            return InteractionResult::Pass;
        };

        context.item_stack.shrink(1);
        context.world.set_block(
            context.hit_result.block_pos,
            REGISTRY
                .blocks
                .copy_matching_properties(old_block_state, waxed_block), // this needs testing
            UpdateFlags::UPDATE_ALL,
        );

        InteractionResult::Success
    }
}
