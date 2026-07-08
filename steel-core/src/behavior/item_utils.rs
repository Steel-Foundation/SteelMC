//! Helpers shared by item behavior implementations.

use steel_registry::item_stack::ItemStack;

use crate::behavior::UseItemContext;
use crate::inventory::lock::ContainerId;

/// Applies vanilla `ItemUtils.createFilledResult`.
pub(crate) fn create_filled_result(
    context: &UseItemContext,
    result_stack: ItemStack,
    limit_creative_stack_size: bool,
) {
    let player = context.player;
    let overflow = context.inv.with_guard(|guard| {
        let inv_id = ContainerId::from_arc(&player.inventory);
        let Some(inv) = guard.get_player_inventory_mut(inv_id) else {
            return result_stack;
        };

        inv.apply_filled_result(
            context.hand,
            result_stack,
            player.has_infinite_materials(),
            limit_creative_stack_size,
        )
    });

    if !overflow.is_empty() {
        player.drop_item(overflow, false, false);
    }
}
