use steel_macros::item_behavior;
use steel_registry::{
    REGISTRY,
    blocks::{
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, EnumProperty},
    },
    data_components::vanilla_components::BLOCKS_ATTACKS,
    level_events::{PARTICLES_SCRAPE, PARTICLES_WAX_OFF},
    sound_events::{ITEM_AXE_SCRAPE, ITEM_AXE_STRIP, ITEM_AXE_WAX_OFF},
};
use steel_utils::{
    math::Axis,
    types::{InteractionHand, UpdateFlags},
};

use crate::behavior::{
    InteractionResult, ItemBehavior, UseOnContext, strippables::get_strippable_variant,
    waxables::get_normal_from_waxed_variant, weathering::previous_copper_stage,
};

const AXIS_PROPERTY: EnumProperty<Axis> = BlockStateProperties::AXIS;

/// Behavior for Axes, when used on wood or logs it turns them into their stripped variants
#[item_behavior]
pub struct AxeItem;

impl ItemBehavior for AxeItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let has_block_item_intent = context.hand == InteractionHand::MainHand
            && context
                .inv
                .inventory()
                .get_offhand_item()
                .has(BLOCKS_ATTACKS)
            && !context.player.is_secondary_use_active();

        if has_block_item_intent {
            return InteractionResult::Pass;
        }

        let old_block_state = context.world.get_block_state(context.hit_result.block_pos);
        let old_block = old_block_state.get_block();

        let pos = context.hit_result.block_pos;

        let (new_block_state, sound_event, level_event) =
            if let Some(new_block) = get_strippable_variant(old_block) {
                let old_axis = old_block_state.get_value(&AXIS_PROPERTY);
                let new_block_state = new_block
                    .default_state()
                    .set_value(&AXIS_PROPERTY, old_axis);

                (new_block_state, ITEM_AXE_STRIP, None)
            } else if let Some(scraped_block) = previous_copper_stage(old_block) {
                let new_block_state = REGISTRY
                    .blocks
                    .copy_matching_properties(old_block_state, scraped_block);

                (new_block_state, ITEM_AXE_SCRAPE, Some(PARTICLES_SCRAPE))
            } else if let Some(unwaxed_block) = get_normal_from_waxed_variant(old_block) {
                let new_block_state = REGISTRY
                    .blocks
                    .copy_matching_properties(old_block_state, unwaxed_block);

                (new_block_state, ITEM_AXE_WAX_OFF, Some(PARTICLES_WAX_OFF))
            } else {
                return InteractionResult::Pass;
            };

        context
            .world
            .set_block(pos, new_block_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);

        context
            .world
            .play_block_sound(sound_event, pos, 1.0, 1.0, Some(context.player.id));

        if let Some(event) = level_event {
            context
                .world
                .level_event(event, pos, 0, Some(context.player.id));
        }

        // TODO: Fire GameEvent::BLOCK_CHANGE for sculk sensors

        let has_infinite_materials = context.player.has_infinite_materials();
        context.inv.item().hurt_and_break(1, has_infinite_materials);

        InteractionResult::Success
    }
}
