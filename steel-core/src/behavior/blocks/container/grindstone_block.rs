use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, EnumProperty},
    },
    items::item::BlockHitResult,
    vanilla_blocks,
};
use steel_utils::{BlockStateId, Direction, translations};
use text_components::TextComponent;

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, InteractionResult, InventoryAccess},
    inventory::menu::kinds::grindstone,
    player::Player,
    world::World,
};

/// Behavior for Grindstone
#[block_behavior]
pub struct GrindstoneBlock {
    block: BlockRef,
}

const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;
const HORIZONTAL_FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;

impl GrindstoneBlock {
    /// Creates a new Grindstone Block Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for GrindstoneBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state().set_value(
            HORIZONTAL_FACING,
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
            move |context| grindstone(inventory, context.container_id, pos, context.world),
        );
        InteractionResult::Success
    }
}
