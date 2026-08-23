use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::items::item::BlockHitResult;
use steel_registry::vanilla_custom_stats;
use steel_utils::{BlockPos, BlockStateId, translations};
use text_components::TextComponent;

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, InteractionResult, InventoryAccess,
        blocks::redstone::face_attached_horizontal_directional_block::FaceAttachedHorizontalDirectionalBlock,
    },
    entity::ai::path::PathComputationType,
    inventory::menu::kinds::grindstone,
    player::Player,
    world::World,
};

/// Vanilla `GrindstoneBlock`.
#[block_behavior]
pub struct GrindstoneBlock {
    face_attached: FaceAttachedHorizontalDirectionalBlock,
}

impl GrindstoneBlock {
    /// Creates a new grindstone block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            face_attached: FaceAttachedHorizontalDirectionalBlock::new(block),
        }
    }
}

impl BlockBehavior for GrindstoneBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.face_attached
            .state_for_placement_checking_support(context, false)
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
            TextComponent::translated(translations::CONTAINER_GRINDSTONE_TITLE.msg()),
            move |context| grindstone(inventory, context.container_id, pos, context.world),
        );
        player.award_custom_stat(&vanilla_custom_stats::INTERACT_WITH_GRINDSTONE);
        InteractionResult::Success
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}
