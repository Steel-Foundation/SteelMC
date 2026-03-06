use steel_registry::{
    blocks::{
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, EnumProperty},
    },
    data_components::vanilla_components::BLOCKS_ATTACKS,
    sound_events::ITEM_AXE_STRIP,
};
use steel_utils::{
    math::Axis,
    types::{InteractionHand, UpdateFlags},
};

use crate::{
    behavior::{
        InteractionResult, ItemBehavior, UseOnContext, strippables::get_strippable_variant,
    },
    entity::LivingEntity,
    inventory::equipment::EquipmentSlot,
};

const AXIS_PROPERTY: EnumProperty<Axis> = BlockStateProperties::AXIS;

/// Behavior for Axes, when used on wood or logs it turns them into their stripped variants
pub struct AxeBehavior;

impl ItemBehavior for AxeBehavior {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let has_block_item_intent = context.hand == InteractionHand::MainHand
            && context.player.get_off_hand_item().has(BLOCKS_ATTACKS)
            && !context.player.is_secondary_use_active();

        if has_block_item_intent {
            return InteractionResult::Pass;
        }

        let old_block_state = context.world.get_block_state(&context.hit_result.block_pos);

        if let Some(new_block) = get_strippable_variant(old_block_state.get_block()) {
            let old_axis = old_block_state.get_value(&AXIS_PROPERTY);
            let new_block_state = new_block
                .default_state()
                .set_value(&AXIS_PROPERTY, old_axis);
            context.world.set_block(
                context.hit_result.block_pos,
                new_block_state,
                UpdateFlags::UPDATE_ALL,
            );

            context
                .player
                .get_item_by_slot(match context.hand {
                    InteractionHand::MainHand => EquipmentSlot::MainHand,
                    InteractionHand::OffHand => EquipmentSlot::OffHand,
                })
                .hurt_and_break(1, context.player.has_infinite_materials());

            return InteractionResult::Success;
        }

        // TODO: scraping and removing wax

        InteractionResult::Pass
    }
}
