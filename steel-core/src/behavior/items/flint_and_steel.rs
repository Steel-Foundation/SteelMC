//! Flint and steel item behavior with portal ignition.

use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::item::ItemBehavior;
use steel_macros::item_behavior;
use steel_registry::vanilla_blocks::FIRE;
use steel_utils::types::UpdateFlags;

/// Behavior for flint and steel items.
#[item_behavior]
pub struct FlintAndSteelItem;

impl ItemBehavior for FlintAndSteelItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let click_pos = context.hit_result.block_pos;
        let fire_pos = click_pos.relative(context.hit_result.direction);

        context.world.set_block(
            fire_pos,
            FIRE.default_state(),
            UpdateFlags::UPDATE_NEIGHBORS,
        );

        let has_infinite_materials = context.player.has_infinite_materials();
        context.inv.item().hurt_and_break(1, has_infinite_materials);

        // TODO: Place fire block at fire_pos if it's air on a solid block
        InteractionResult::Pass
    }
}
