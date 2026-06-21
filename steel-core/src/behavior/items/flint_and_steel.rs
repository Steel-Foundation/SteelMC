//! Flint and steel item behavior with portal ignition.

use std::sync::Arc;

use crate::behavior::InventoryAccess;
use crate::behavior::blocks::FireBlock;
use crate::behavior::context::InteractionResult;
use crate::behavior::item::ItemBehavior;
use crate::player::Player;
use crate::world::World;
use steel_macros::item_behavior;
use steel_registry::items::item::BlockHitResult;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{
    blocks::{block_state_ext::BlockStateExt, properties::BlockStateProperties},
    sound_events, vanilla_game_events,
};
use steel_utils::types::{InteractionHand, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::entity::Entity;
use crate::world::game_event_context::GameEventContext;

/// Behavior for flint and steel items.
#[item_behavior]
pub struct FlintAndSteelItem;

impl ItemBehavior for FlintAndSteelItem {
    fn use_on(
        &self,
        player: &Player,
        _hand: InteractionHand,
        hit_result: BlockHitResult,
        world: &Arc<World>,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let click_pos = hit_result.block_pos;
        let clicked_state = world.get_block_state(click_pos);
        if try_light_block(
            player,
            world,
            click_pos,
            clicked_state,
            &sound_events::ITEM_FLINTANDSTEEL_USE,
            flint_and_steel_pitch(),
        ) {
            let has_infinite_materials = player.has_infinite_materials();
            inv.with_item(|item| item.hurt_and_break(1, has_infinite_materials));
            return InteractionResult::Success;
        }

        let fire_pos = click_pos.relative(hit_result.direction);
        let (yaw, _) = player.rotation();
        let forward_dir = Direction::from_yaw(yaw);

        if !FireBlock::can_be_placed_at(world, fire_pos, forward_dir) {
            return InteractionResult::Fail;
        }

        world.play_block_sound(
            &sound_events::ITEM_FLINTANDSTEEL_USE,
            fire_pos,
            1.0,
            rand::random::<f32>() * 0.4 + 0.8,
            Some(player.id()),
        );

        world.set_block(
            fire_pos,
            FireBlock::get_state(world.as_ref(), fire_pos),
            UpdateFlags::UPDATE_ALL,
        );
        world.game_event(
            &vanilla_game_events::BLOCK_PLACE,
            click_pos,
            &GameEventContext::new(Some(player), None),
        );

        let has_infinite_materials = player.has_infinite_materials();
        inv.with_item(|item| item.hurt_and_break(1, has_infinite_materials));

        InteractionResult::Success
    }
}

/// Behavior for fire charge items.
#[item_behavior]
pub struct FireChargeItem;

impl ItemBehavior for FireChargeItem {
    fn use_on(
        &self,
        player: &Player,
        _hand: InteractionHand,
        hit_result: BlockHitResult,
        world: &Arc<World>,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let click_pos = hit_result.block_pos;
        let clicked_state = world.get_block_state(click_pos);
        if try_light_block(
            player,
            world,
            click_pos,
            clicked_state,
            &sound_events::ITEM_FIRECHARGE_USE,
            fire_charge_pitch(),
        ) {
            inv.with_item(|item| item.shrink(1));
            return InteractionResult::Success;
        }

        let fire_pos = click_pos.relative(hit_result.direction);
        let (yaw, _) = player.rotation();
        let forward_dir = Direction::from_yaw(yaw);

        if !FireBlock::can_be_placed_at(world, fire_pos, forward_dir) {
            return InteractionResult::Fail;
        }

        world.play_block_sound(
            &sound_events::ITEM_FIRECHARGE_USE,
            fire_pos,
            1.0,
            fire_charge_pitch(),
            Some(player.id()),
        );

        world.set_block(
            fire_pos,
            FireBlock::get_state(world.as_ref(), fire_pos),
            UpdateFlags::UPDATE_ALL,
        );
        world.game_event(
            &vanilla_game_events::BLOCK_PLACE,
            fire_pos,
            &GameEventContext::new(Some(player), None),
        );

        inv.with_item(|item| item.shrink(1));

        InteractionResult::Success
    }
}

fn try_light_block(
    player: &Player,
    world: &Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
    sound: SoundEventRef,
    pitch: f32,
) -> bool {
    if !can_light(state) {
        return false;
    }

    world.play_block_sound(sound, pos, 1.0, pitch, Some(player.id()));
    world.set_block(
        pos,
        state.set_value(&BlockStateProperties::LIT, true),
        UpdateFlags::UPDATE_ALL_IMMEDIATE,
    );
    world.game_event(
        &vanilla_game_events::BLOCK_CHANGE,
        pos,
        &GameEventContext::new(Some(player), None),
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
    if block.has_tag(&BlockTag::CAMPFIRES) {
        return state.try_get_value(&BlockStateProperties::WATERLOGGED) == Some(false);
    }

    if block.has_tag(&BlockTag::CANDLES) {
        return state.try_get_value(&BlockStateProperties::WATERLOGGED) == Some(false);
    }

    block.has_tag(&BlockTag::CANDLE_CAKES)
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
