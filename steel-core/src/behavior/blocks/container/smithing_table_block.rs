use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::items::item::BlockHitResult;
use steel_registry::vanilla_custom_stats;
use steel_utils::{BlockPos, BlockStateId, translations};
use text_components::TextComponent;

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, InteractionResult, InventoryAccess},
    inventory::menu::kinds::smithing,
    player::Player,
    world::World,
};

/// Vanilla `SmithingTableBlock`.
#[block_behavior]
pub struct SmithingTableBlock {
    block: BlockRef,
}

impl SmithingTableBlock {
    /// Creates a new smithing table block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SmithingTableBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn use_without_item(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let inventory = player.inventory.clone();
        player.open_menu(
            TextComponent::translated(translations::CONTAINER_UPGRADE.msg()),
            move |context| smithing(inventory, context.container_id, pos, context.world),
        );
        player.award_custom_stat(&vanilla_custom_stats::INTERACT_WITH_SMITHING_TABLE);
        InteractionResult::Success
    }
}
