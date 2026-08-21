use std::sync::Arc;

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::inventory;
use crate::inventory::menu::kinds::loom;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, EnumProperty};
use steel_utils::{BlockPos, BlockStateId, translations};
use text_components::TextComponent;

#[block_behavior]
pub struct LoomBlock {
    block: BlockRef,
}

const FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;

impl LoomBlock {
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for LoomBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let facing = context.get_nearest_looking_direction().opposite();

        Some(self.block.default_state().set_value(FACING, facing))
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<crate::world::World>,
        pos: BlockPos,
        player: &crate::player::Player,
        hit_result: &BlockHitResult,
        inv: &mut crate::behavior::InventoryAccess,
    ) -> crate::behavior::InteractionResult {
        let inventory = player.inventory.clone();
        player.open_menu(
            TextComponent::translated(translations::CONTAINER_LOOM.msg()),
            move |context| loom(inventory, context.container_id, pos),
        );
        InteractionResult::Success
    }
}
