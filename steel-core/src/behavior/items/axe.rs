use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::{
    REGISTRY,
    blocks::{
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, EnumProperty},
    },
    data_components::vanilla_components::BLOCKS_ATTACKS,
    items::item::BlockHitResult,
    level_events::{PARTICLES_SCRAPE, PARTICLES_WAX_OFF},
    sound_events::{ITEM_AXE_SCRAPE, ITEM_AXE_STRIP, ITEM_AXE_WAX_OFF},
    vanilla_game_events,
};
use steel_utils::{
    axis::Axis,
    types::{InteractionHand, UpdateFlags},
};

use crate::{
    behavior::{
        InteractionResult, InventoryAccess, ItemBehavior, strippables::get_strippable_variant,
        waxables::get_normal_from_waxed_variant, weathering::previous_copper_stage,
    },
    entity::Entity,
    player::Player,
    world::{World, game_event_context::GameEventContext},
};

use super::copper_chest_events::emit_connected_chest_block_change;

const AXIS_PROPERTY: EnumProperty<Axis> = BlockStateProperties::AXIS;

/// Behavior for Axes, when used on wood or logs it turns them into their stripped variants
#[item_behavior]
pub struct AxeItem;

impl ItemBehavior for AxeItem {
    fn use_on(
        &self,
        player: &Player,
        hand: InteractionHand,
        hit_result: BlockHitResult,
        world: &Arc<World>,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let has_block_item_intent = hand == InteractionHand::MainHand
            && inv.with_inventory(|inv| inv.get_offhand_item().has(BLOCKS_ATTACKS))
            && !player.is_secondary_use_active();

        if has_block_item_intent {
            return InteractionResult::Pass;
        }

        let old_block_state = world.get_block_state(hit_result.block_pos);
        let old_block = old_block_state.get_block();

        let pos = hit_result.block_pos;

        let (new_block_state, sound_event, level_event) =
            if let Some(new_block) = get_strippable_variant(old_block) {
                let old_axis = old_block_state.get_value(&AXIS_PROPERTY);
                let new_block_state = new_block
                    .default_state()
                    .set_value(&AXIS_PROPERTY, old_axis);

                (new_block_state, &ITEM_AXE_STRIP, None)
            } else if let Some(scraped_block) = previous_copper_stage(old_block) {
                let new_block_state = REGISTRY
                    .blocks
                    .copy_matching_properties(old_block_state, scraped_block);

                (new_block_state, &ITEM_AXE_SCRAPE, Some(PARTICLES_SCRAPE))
            } else if let Some(unwaxed_block) = get_normal_from_waxed_variant(old_block) {
                let new_block_state = REGISTRY
                    .blocks
                    .copy_matching_properties(old_block_state, unwaxed_block);

                (new_block_state, &ITEM_AXE_WAX_OFF, Some(PARTICLES_WAX_OFF))
            } else {
                return InteractionResult::Pass;
            };

        world.set_block(pos, new_block_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);

        world.play_block_sound(sound_event, pos, 1.0, 1.0, Some(player.id()));

        if let Some(event) = level_event {
            world.level_event(event, pos, 0, Some(player.id()));
            emit_connected_chest_block_change(world, pos, old_block_state, player, Some(event));
        }

        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(Some(player), Some(new_block_state)),
        );

        let has_infinite_materials = player.has_infinite_materials();
        inv.with_item(|item| item.hurt_and_break(1, has_infinite_materials));

        InteractionResult::Success
    }
}
