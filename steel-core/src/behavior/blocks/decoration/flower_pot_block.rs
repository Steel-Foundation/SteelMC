use steel_macros::block_behavior;
use steel_registry::blocks::{
    BlockRef,
    block_state_ext::BlockStateExt,
    properties::Direction,
};
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{
    BlockBehavior, BlockPlaceContext, InteractionResult,
    context::{BlockHitResult, InventoryAccess},
};
use crate::entity::Player;
use crate::world::{LevelReader, ScheduledTickAccess, World};

#[block_behavior]
pub struct FlowerPotBlock {
    block: BlockRef,
    potted: BlockRef,
}

#[must_use]
pub const fn new(block: BlockRef, potted: BlockRef) -> Self {
    Self { block, potted }
}

impl BlockBehavior for FlowerPotBlock {
    fn use_item_on(
        &self,
        _state: BlockStateId,
        world: &World,
        _pos: BlockPos,
        player: &Player,
        _hand: InteractionHand,
        _hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        if self.is_empty() {
            // Empty pot — no held block maps to a potted variant without a
            // static POTTED_BY_CONTENT map (vanilla fills this at construction).
            InteractionResult::TryEmptyHandInteraction
        } else if self.potted == vanilla_blocks::AIR.default_state() {
            // Pot exists but is empty (shouldn't happen after init, but be safe)
            InteractionResult::TryEmptyHandInteraction
        } else {
            // Pot already has content → remove it and drop the item to the player
            world.set_block(pos, vanilla_blocks::FLOWER_POT.default_state(), UpdateFlags::UPDATE_ALL);
            if let Some(player) = player {
                let _ = player.add_item_or_drop(self.potted);
            }
            InteractionResult::Success
        }
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        // Pick-block: return the potted plant item if pot non-empty, else None.
        if self.is_empty() {
            None
        } else {
            Some(ItemStack::new(self.potted))
        }
    }

    fn is_pathfindable(&self, _state: BlockStateId, _computation_type: PathComputationType) -> bool {
        false
    }

    fn can_survive(&self, _state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        true
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        // Vanilla: only breaks when direction == DOWN && !canSurvive.
        // FlowerPotBlock does not override canSurvive (default returns true),
        // so this never triggers — pots float when support removed, matching
        // vanilla behavior. Mirroring the structure is sufficient.
        if _direction == Direction::Down && !state.can_survive(world, pos) {
            vanilla_blocks::AIR.default_state()
        } else {
            state
        }
    }
}