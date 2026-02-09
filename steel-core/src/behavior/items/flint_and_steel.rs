//! Flint and steel item behavior with portal ignition.

use steel_registry::vanilla_blocks::FIRE;
use steel_utils::types::UpdateFlags;
use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::item::ItemBehavior;

/// Behavior for flint and steel items.
pub struct FlintAndSteelBehavior;

impl ItemBehavior for FlintAndSteelBehavior {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let click_pos = context.hit_result.block_pos;
        let fire_pos = click_pos.relative(context.hit_result.direction);

        context.world.set_block(fire_pos, FIRE.default_state(),UpdateFlags::UPDATE_NEIGHBORS);

        // TODO: Place fire block at fire_pos if it's air on a solid block
        InteractionResult::Pass
    }
}
