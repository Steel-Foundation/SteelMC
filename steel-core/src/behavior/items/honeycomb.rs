use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::{
    REGISTRY, blocks::block_state_ext::BlockStateExt, items::item::BlockHitResult, level_events,
    vanilla_game_events,
};
use steel_utils::types::{InteractionHand, UpdateFlags};

use crate::{
    behavior::{
        InteractionResult, InventoryAccess, ItemBehavior, waxables::get_waxed_from_normal_variant,
    },
    block_entity::{BlockEntity, entities::SignBlockEntity},
    entity::Entity,
    player::Player,
    world::{World, game_event_context::GameEventContext},
};

use super::copper_chest_events::emit_connected_chest_block_change;

/// Behavior for the honeycomb item. Waxes copper blocks and signs.
#[item_behavior]
pub struct HoneycombItem;

impl ItemBehavior for HoneycombItem {
    fn use_on(
        &self,
        player: &Player,
        _hand: InteractionHand,
        hit_result: BlockHitResult,
        world: &Arc<World>,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let pos = hit_result.block_pos;

        // Try block waxing first
        let old_block_state = world.get_block_state(pos);
        if let Some(waxed_block) = get_waxed_from_normal_variant(old_block_state.get_block()) {
            inv.with_item(|item| item.shrink(1));
            // TODO: trigger CriteriaTriggers.ITEM_USED_ON_BLOCK advancement
            let waxed_state = REGISTRY
                .blocks
                .copy_matching_properties(old_block_state, waxed_block);
            world.set_block(pos, waxed_state, UpdateFlags::UPDATE_ALL);
            world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                pos,
                &GameEventContext::new(Some(player), Some(waxed_state)),
            );
            world.level_event(
                level_events::PARTICLES_AND_SOUND_WAX_ON,
                pos,
                0,
                Some(player.id()),
            );
            emit_connected_chest_block_change(
                world,
                pos,
                old_block_state,
                player,
                Some(level_events::PARTICLES_AND_SOUND_WAX_ON),
            );
            return InteractionResult::Success;
        }

        // Fall through to sign waxing
        let Some(block_entity) = world.get_block_entity(pos) else {
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
        inv.with_item(|item| item.shrink(1));
        world.level_event(
            level_events::PARTICLES_AND_SOUND_WAX_ON,
            pos,
            0,
            Some(player.id()),
        );
        InteractionResult::Success
    }
}
