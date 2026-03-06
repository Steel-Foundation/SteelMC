use std::ptr;

use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, EnumProperty},
    },
    data_components::vanilla_components::BLOCKS_ATTACKS,
    sound_events::ITEM_AXE_STRIP,
    vanilla_blocks::{self},
};
use steel_utils::{
    math::Axis,
    types::{InteractionHand, UpdateFlags},
};

use crate::{
    behavior::{InteractionResult, ItemBehavior, UseOnContext},
    entity::LivingEntity,
    inventory::equipment::EquipmentSlot,
};

fn get_stripped_variant(block: BlockRef) -> Option<BlockRef> {
    match block {
        b if ptr::eq(b, vanilla_blocks::OAK_WOOD) => Some(vanilla_blocks::STRIPPED_OAK_WOOD),
        b if ptr::eq(b, vanilla_blocks::OAK_LOG) => Some(vanilla_blocks::STRIPPED_OAK_LOG),
        b if ptr::eq(b, vanilla_blocks::DARK_OAK_WOOD) => {
            Some(vanilla_blocks::STRIPPED_DARK_OAK_WOOD)
        }
        b if ptr::eq(b, vanilla_blocks::DARK_OAK_LOG) => {
            Some(vanilla_blocks::STRIPPED_DARK_OAK_LOG)
        }
        b if ptr::eq(b, vanilla_blocks::PALE_OAK_WOOD) => {
            Some(vanilla_blocks::STRIPPED_PALE_OAK_WOOD)
        }
        b if ptr::eq(b, vanilla_blocks::PALE_OAK_LOG) => {
            Some(vanilla_blocks::STRIPPED_PALE_OAK_LOG)
        }
        b if ptr::eq(b, vanilla_blocks::ACACIA_WOOD) => Some(vanilla_blocks::STRIPPED_ACACIA_WOOD),
        b if ptr::eq(b, vanilla_blocks::ACACIA_LOG) => Some(vanilla_blocks::STRIPPED_ACACIA_LOG),
        b if ptr::eq(b, vanilla_blocks::CHERRY_WOOD) => Some(vanilla_blocks::STRIPPED_CHERRY_WOOD),
        b if ptr::eq(b, vanilla_blocks::CHERRY_LOG) => Some(vanilla_blocks::STRIPPED_CHERRY_LOG),
        b if ptr::eq(b, vanilla_blocks::BIRCH_WOOD) => Some(vanilla_blocks::STRIPPED_BIRCH_WOOD),
        b if ptr::eq(b, vanilla_blocks::BIRCH_LOG) => Some(vanilla_blocks::STRIPPED_BIRCH_LOG),
        b if ptr::eq(b, vanilla_blocks::JUNGLE_WOOD) => Some(vanilla_blocks::STRIPPED_JUNGLE_WOOD),
        b if ptr::eq(b, vanilla_blocks::JUNGLE_LOG) => Some(vanilla_blocks::STRIPPED_JUNGLE_LOG),
        b if ptr::eq(b, vanilla_blocks::SPRUCE_WOOD) => Some(vanilla_blocks::STRIPPED_SPRUCE_WOOD),
        b if ptr::eq(b, vanilla_blocks::SPRUCE_LOG) => Some(vanilla_blocks::STRIPPED_SPRUCE_LOG),
        b if ptr::eq(b, vanilla_blocks::WARPED_STEM) => Some(vanilla_blocks::STRIPPED_WARPED_STEM),
        b if ptr::eq(b, vanilla_blocks::WARPED_HYPHAE) => {
            Some(vanilla_blocks::STRIPPED_WARPED_HYPHAE)
        }
        b if ptr::eq(b, vanilla_blocks::CRIMSON_STEM) => {
            Some(vanilla_blocks::STRIPPED_CRIMSON_STEM)
        }
        b if ptr::eq(b, vanilla_blocks::CRIMSON_HYPHAE) => {
            Some(vanilla_blocks::STRIPPED_CRIMSON_HYPHAE)
        }
        b if ptr::eq(b, vanilla_blocks::MANGROVE_WOOD) => {
            Some(vanilla_blocks::STRIPPED_MANGROVE_WOOD)
        }
        b if ptr::eq(b, vanilla_blocks::MANGROVE_LOG) => {
            Some(vanilla_blocks::STRIPPED_MANGROVE_LOG)
        }
        b if ptr::eq(b, vanilla_blocks::BAMBOO_BLOCK) => {
            Some(vanilla_blocks::STRIPPED_BAMBOO_BLOCK)
        }
        _ => None,
    }
}

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

        if let Some(new_block) = get_stripped_variant(old_block_state.get_block()) {
            let old_axis = old_block_state.get_value(&AXIS_PROPERTY);
            let new_block_state = new_block
                .default_state()
                .set_value(&AXIS_PROPERTY, old_axis);
            context.world.set_block(
                context.hit_result.block_pos,
                new_block_state,
                UpdateFlags::UPDATE_ALL,
            );

            context.world.play_block_sound(
                ITEM_AXE_STRIP,
                context.hit_result.block_pos,
                1.0,
                1.0,
                None,
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
