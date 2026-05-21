//! Flint and steel item behavior with portal ignition.

use crate::behavior::blocks::FireBlock;
use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::item::ItemBehavior;
use steel_macros::item_behavior;
use steel_registry::{
    REGISTRY, TaggedRegistryExt,
    blocks::{block_state_ext::BlockStateExt, properties::BlockStateProperties},
    sound_events, vanilla_block_tags, vanilla_game_events,
};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::world::game_event_context::GameEventContext;

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

        context.world.set_block(
            fire_pos,
            FireBlock::get_state(context.world.as_ref(), fire_pos),
            UpdateFlags::UPDATE_ALL,
        );
        context.world.game_event(
            &vanilla_game_events::BLOCK_PLACE,
            click_pos,
            &GameEventContext::new(Some(context.player), None),
        );

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

        context.world.set_block(
            fire_pos,
            FireBlock::get_state(context.world.as_ref(), fire_pos),
            UpdateFlags::UPDATE_ALL,
        );
        context.world.game_event(
            &vanilla_game_events::BLOCK_PLACE,
            fire_pos,
            &GameEventContext::new(Some(context.player), None),
        );

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
    context.world.game_event(
        &vanilla_game_events::BLOCK_CHANGE,
        pos,
        &GameEventContext::new(Some(context.player), None),
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
    if REGISTRY
        .blocks
        .is_in_tag(block, &vanilla_block_tags::CAMPFIRES_TAG)
    {
        return state.try_get_value(&BlockStateProperties::WATERLOGGED) == Some(false);
    }

    if REGISTRY
        .blocks
        .is_in_tag(block, &vanilla_block_tags::CANDLES_TAG)
    {
        return state.try_get_value(&BlockStateProperties::WATERLOGGED) == Some(false);
    }

    REGISTRY
        .blocks
        .is_in_tag(block, &vanilla_block_tags::CANDLE_CAKES_TAG)
}

fn flint_and_steel_pitch() -> f32 {
    rand::random::<f32>() * 0.4 + 0.8
}

fn fire_charge_pitch() -> f32 {
    (rand::random::<f32>() - rand::random::<f32>()) * 0.2 + 1.0
}

#[cfg(test)]
mod tests {
    use steel_registry::{
        blocks::{block_state_ext::BlockStateExt, properties::BlockStateProperties},
        test_support::init_test_registry,
        vanilla_blocks,
    };

    use super::can_light;

    #[test]
    fn can_light_rejects_waterlogged_campfires_and_candles() {
        init_test_registry();

        let waterlogged_campfire = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(&BlockStateProperties::LIT, false)
            .set_value(&BlockStateProperties::WATERLOGGED, true);
        let dry_campfire =
            waterlogged_campfire.set_value(&BlockStateProperties::WATERLOGGED, false);

        let waterlogged_candle = vanilla_blocks::CANDLE
            .default_state()
            .set_value(&BlockStateProperties::LIT, false)
            .set_value(&BlockStateProperties::WATERLOGGED, true);
        let dry_candle = waterlogged_candle.set_value(&BlockStateProperties::WATERLOGGED, false);

        assert!(!can_light(waterlogged_campfire));
        assert!(can_light(dry_campfire));
        assert!(!can_light(waterlogged_candle));
        assert!(can_light(dry_candle));
    }

    #[test]
    fn can_light_accepts_unlit_candle_cakes() {
        init_test_registry();

        let unlit_candle_cake = vanilla_blocks::CANDLE_CAKE
            .default_state()
            .set_value(&BlockStateProperties::LIT, false);
        let lit_candle_cake = unlit_candle_cake.set_value(&BlockStateProperties::LIT, true);

        assert!(can_light(unlit_candle_cake));
        assert!(!can_light(lit_candle_cake));
    }
}
