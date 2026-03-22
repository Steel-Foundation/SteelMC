//! Flint and steel item behavior with portal ignition.

use crate::behavior::blocks::FireBlock;
use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::item::ItemBehavior;
use steel_macros::item_behavior;
use steel_registry::sound_events;
use steel_registry::vanilla_blocks::FIRE;
use steel_utils::Direction;
use steel_utils::types::UpdateFlags;

/// Behavior for flint and steel items.
#[item_behavior]
pub struct FlintAndSteelItem;

impl ItemBehavior for FlintAndSteelItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        // TODO: light campfires, candles, and candle cakes (set LIT=true) before fire placement

        let click_pos = context.hit_result.block_pos;
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
        context.inv.item().hurt_and_break(1, has_infinite_materials);

        InteractionResult::Success
    }
}
