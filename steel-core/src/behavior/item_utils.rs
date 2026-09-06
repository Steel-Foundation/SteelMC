//! Helpers shared by item behavior implementations.

use steel_registry::item_stack::ItemStack;
use steel_registry::items::item::BlockHitResult;

use crate::behavior::UseItemContext;
use crate::inventory::lock::ContainerId;
use crate::player::Player;
use crate::player::player_inventory::PlayerInventory;
use crate::world::{ClipBlockShape, ClipFluid, World};

/// Vanilla `Item.getPlayerPOVHitResult`.
#[must_use]
pub(crate) fn get_player_pov_hit_result(
    world: &World,
    player: &Player,
    fluid: ClipFluid,
) -> BlockHitResult {
    let (from, to) = player.get_ray_endpoints();
    let hit = world.clip(from, to, ClipBlockShape::Outline, fluid);
    BlockHitResult {
        location: hit.location,
        direction: hit.direction,
        block_pos: hit.block_pos,
        miss: hit.miss,
        inside: hit.inside,
        world_border_hit: hit.world_border_hit,
    }
}

/// Applies vanilla `ItemUtils.createFilledResult`.
pub(crate) fn create_filled_result(
    context: &UseItemContext,
    result_stack: ItemStack,
    limit_creative_stack_size: bool,
) {
    let player = context.player;
    let overflow = context.inv.with_guard(|guard| {
        let inv_id = ContainerId::from_arc(&player.inventory);
        let Some(inv) = guard.get_typed_mut::<PlayerInventory>(inv_id) else {
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
        let _ = player.drop_item(overflow, false, false);
    }
}
