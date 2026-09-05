use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{blocks::BlockRef, items::item::BlockHitResult};
use steel_utils::{BlockPos, BlockStateId, Direction, translations};
use text_components::TextComponent;

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, InteractionResult, InventoryAccess,
        blocks::face_attached_horizontal_directional_block::FaceAttachedHorizontalDirectionalBlock,
    },
    inventory::menu::kinds::grindstone,
    player::Player,
    world::{LevelReader, ScheduledTickAccess, World},
};

/// Behavior for Grindstone
#[block_behavior]
pub struct GrindstoneBlock {
    face_attached: FaceAttachedHorizontalDirectionalBlock,
}

impl GrindstoneBlock {
    /// Creates a new Grindstone Block Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            face_attached: FaceAttachedHorizontalDirectionalBlock::new(block),
        }
    }
}

impl BlockBehavior for GrindstoneBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.face_attached.state_for_placement(context)
    }

    /// Vanilla `GrindstoneBlock.canSurvive` overrides the face-attached rule and
    /// always returns true, so a grindstone needs no sturdy support and never pops
    /// off — it can be placed on anything, including another grindstone.
    fn can_survive(&self, _state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        true
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        FaceAttachedHorizontalDirectionalBlock::update_shape(state, world, pos, direction)
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
            TextComponent::translated(translations::CONTAINER_REPAIR.msg()),
            move |context| grindstone(inventory, context.container_id, pos, context.world),
        );
        InteractionResult::Success
    }
}
