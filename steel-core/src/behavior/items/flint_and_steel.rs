//! Flint and steel item behavior with portal ignition.

use crate::behavior::blocks::FireBlock;
use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::item::ItemBehavior;
use steel_macros::item_behavior;
use steel_registry::{
    REGISTRY, TaggedRegistryExt,
    blocks::{block_state_ext::BlockStateExt, properties::BlockStateProperties},
    sound_events, vanilla_block_tags,
    vanilla_blocks::FIRE,
};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

/// Behavior for flint and steel items.
#[item_behavior]
pub struct FlintAndSteelItem;

impl ItemBehavior for FlintAndSteelItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let click_pos = context.hit_result.block_pos;
        let clicked_state = context.world.get_block_state(click_pos);
        if try_light_block(
            context,
            click_pos,
            clicked_state,
            sound_events::ITEM_FLINTANDSTEEL_USE,
            flint_and_steel_pitch(),
        ) {
            let has_infinite_materials = context.player.has_infinite_materials();
            context
                .inv
                .with_item(|item| item.hurt_and_break(1, has_infinite_materials));
            return InteractionResult::Success;
        }

        let fire_pos = click_pos.relative(context.hit_result.direction);
        let (yaw, _) = context.player.rotation.load();
        let forward_dir = Direction::from_yaw(yaw);

        if !FireBlock::can_be_placed_at(context.world, fire_pos, forward_dir) {
            return InteractionResult::Fail;
        }

        context.world.play_block_sound(
            sound_events::ITEM_FLINTANDSTEEL_USE,
            fire_pos,
            1.0,
            rand::random::<f32>() * 0.4 + 0.8,
            Some(context.player.id),
        );

        // TODO: use BaseFireBlock.getState() equivalent to select soul fire vs regular fire
        context
            .world
            .set_block(fire_pos, FIRE.default_state(), UpdateFlags::UPDATE_ALL);

        let has_infinite_materials = context.player.has_infinite_materials();
        context
            .inv
            .with_item(|item| item.hurt_and_break(1, has_infinite_materials));

        InteractionResult::Success
    }
}

/// Behavior for fire charge items.
#[item_behavior]
pub struct FireChargeItem;

impl ItemBehavior for FireChargeItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let click_pos = context.hit_result.block_pos;
        let clicked_state = context.world.get_block_state(click_pos);
        if try_light_block(
            context,
            click_pos,
            clicked_state,
            sound_events::ITEM_FIRECHARGE_USE,
            fire_charge_pitch(),
        ) {
            context.inv.with_item(|item| item.shrink(1));
            return InteractionResult::Success;
        }

        let fire_pos = click_pos.relative(context.hit_result.direction);
        let (yaw, _) = context.player.rotation.load();
        let forward_dir = Direction::from_yaw(yaw);

        if !FireBlock::can_be_placed_at(context.world, fire_pos, forward_dir) {
            return InteractionResult::Fail;
        }

        context.world.play_block_sound(
            sound_events::ITEM_FIRECHARGE_USE,
            fire_pos,
            1.0,
            fire_charge_pitch(),
            Some(context.player.id),
        );

        // TODO: use BaseFireBlock.getState() equivalent to select soul fire vs regular fire
        context
            .world
            .set_block(fire_pos, FIRE.default_state(), UpdateFlags::UPDATE_ALL);

        context.inv.with_item(|item| item.shrink(1));

        InteractionResult::Success
    }
}

fn try_light_block(
    context: &UseOnContext<'_>,
    pos: BlockPos,
    state: BlockStateId,
    sound: i32,
    pitch: f32,
) -> bool {
    if !can_light(state) {
        return false;
    }

    context
        .world
        .play_block_sound(sound, pos, 1.0, pitch, Some(context.player.id));
    context.world.set_block(
        pos,
        state.set_value(&BlockStateProperties::LIT, true),
        UpdateFlags::UPDATE_ALL_IMMEDIATE,
    );

    true
}

fn can_light(state: BlockStateId) -> bool {
    let Some(lit) = state.try_get_value(&BlockStateProperties::LIT) else {
        return false;
    };
    if lit {
        return false;
    }

    let block = state.get_block();
    REGISTRY
        .blocks
        .is_in_tag(block, &vanilla_block_tags::CAMPFIRES_TAG)
        || REGISTRY
            .blocks
            .is_in_tag(block, &vanilla_block_tags::CANDLES_TAG)
        || REGISTRY
            .blocks
            .is_in_tag(block, &vanilla_block_tags::CANDLE_CAKES_TAG)
}

fn flint_and_steel_pitch() -> f32 {
    rand::random::<f32>() * 0.4 + 0.8
}

fn fire_charge_pitch() -> f32 {
    (rand::random::<f32>() - rand::random::<f32>()) * 0.2 + 1.0
}
