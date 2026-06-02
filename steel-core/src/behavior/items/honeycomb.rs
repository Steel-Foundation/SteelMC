use steel_macros::item_behavior;
use steel_registry::{
    REGISTRY, blocks::block_state_ext::BlockStateExt, level_events, vanilla_game_events,
};
use steel_utils::types::UpdateFlags;

use crate::{
    behavior::{
        InteractionResult, ItemBehavior, UseOnContext, waxables::get_waxed_from_normal_variant,
    },
    block_entity::{BlockEntity, entities::SignBlockEntity},
    entity::Entity,
    world::game_event_context::GameEventContext,
};

use super::copper_chest_events::emit_connected_chest_block_change;

/// Behavior for the honeycomb item. Waxes copper blocks and signs.
#[item_behavior]
pub struct HoneycombItem;

impl ItemBehavior for HoneycombItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let pos = context.hit_result.block_pos;

        // Try block waxing first
        let old_block_state = context.world.get_block_state(pos);
        if let Some(waxed_block) = get_waxed_from_normal_variant(old_block_state.get_block()) {
            context.inv.with_item(|item| item.shrink(1));
            // TODO: trigger CriteriaTriggers.ITEM_USED_ON_BLOCK advancement
            let waxed_state = REGISTRY
                .blocks
                .copy_matching_properties(old_block_state, waxed_block);
            context
                .world
                .set_block(pos, waxed_state, UpdateFlags::UPDATE_ALL);
            context.world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                pos,
                &GameEventContext::new(Some(context.player), Some(waxed_state)),
            );
            context.world.level_event(
                level_events::PARTICLES_AND_SOUND_WAX_ON,
                pos,
                0,
                Some(context.player.id()),
            );
            emit_connected_chest_block_change(
                context.world,
                pos,
                old_block_state,
                context.player,
                Some(level_events::PARTICLES_AND_SOUND_WAX_ON),
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
        context.inv.with_item(|item| item.shrink(1));
        context.world.level_event(
            level_events::PARTICLES_AND_SOUND_WAX_ON,
            pos,
            0,
            Some(context.player.id()),
        );
        InteractionResult::Success
    }
}
