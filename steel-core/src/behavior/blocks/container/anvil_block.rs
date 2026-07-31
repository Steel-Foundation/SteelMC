use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt, properties::BlockStateProperties},
    items::item::BlockHitResult,
    vanilla_blocks,
};
use steel_utils::{BlockStateId, translations};
use text_components::TextComponent;

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, InteractionResult, InventoryAccess},
    inventory::menu::kinds::anvil,
    player::Player,
    world::World,
};

/// Behavior for Anvils
#[block_behavior]
pub struct AnvilBlock {
    block: BlockRef,
}

impl AnvilBlock {
    /// Creates a new Anvil Block Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Returns the next damage level block state for anvil `BlockStates`
    #[must_use]
    pub fn damage(state: BlockStateId) -> Option<BlockStateId> {
        let block = state.get_block();
        if block == &vanilla_blocks::ANVIL {
            Some(
                vanilla_blocks::CHIPPED_ANVIL
                    .default_state()
                    .copy_value(&BlockStateProperties::FACING, &state),
            )
        } else if block == &vanilla_blocks::CHIPPED_ANVIL {
            Some(
                vanilla_blocks::DAMAGED_ANVIL
                    .default_state()
                    .copy_value(&BlockStateProperties::FACING, &state),
            )
        } else {
            None
        }
    }
}

impl BlockBehavior for AnvilBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state().set_value(
            &BlockStateProperties::HORIZONTAL_FACING,
            context.horizontal_direction().rotate_y_clockwise(),
        ))
    }

    fn use_without_item(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        pos: steel_utils::BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let inventory = player.inventory.clone();
        player.open_menu(
            TextComponent::translated(translations::CONTAINER_REPAIR.msg()),
            move |context| anvil(inventory, context.container_id, pos, context.world),
        );
        InteractionResult::Success
    }
}
