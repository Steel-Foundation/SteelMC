use crate::behavior::blocks::WallSkullBlock;
use crate::behavior::blocks::decoration::skull::abstract_skull_block::AbstractSkullBlock;
use crate::behavior::{BlockBehavior, BlockEntityCreation, BlockLootContext, BlockPlaceContext};
use crate::block_entity::SharedBlockEntity;
use crate::entity::ai::path::PathComputationType;
use crate::world::World;
use std::sync::{Arc, Weak};
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::item_stack::ItemStack;
use steel_utils::{BlockPos, BlockStateId};

/// Behavior for wither wall skull blocks.
#[block_behavior]
pub struct WitherWallSkullBlock {
    base: WallSkullBlock,
}

impl WitherWallSkullBlock {
    /// Creates a new wither wall skull block behavior for the given block.
    pub const fn new(block: BlockRef) -> Self {
        Self {
            base: WallSkullBlock::new(block),
        }
    }
}

impl BlockBehavior for WitherWallSkullBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.base.get_state_for_placement(context)
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source_block: BlockRef,
        moved_by_piston: bool,
    ) {
        self.base
            .handle_neighbor_changed(state, world, pos, source_block, moved_by_piston);
    }

    fn is_pathfindable(&self, state: BlockStateId, computation_type: PathComputationType) -> bool {
        self.base.is_pathfindable(state, computation_type)
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        self.base.new_block_entity(level, pos, state)
    }

    fn get_clone_item_stack(
        &self,
        block: BlockRef,
        state: BlockStateId,
        block_entity: Option<SharedBlockEntity>,
        include_data: bool,
    ) -> Option<ItemStack> {
        self.base
            .get_clone_item_stack(block, state, block_entity, include_data)
    }

    fn get_drops(
        &self,
        state: BlockStateId,
        context: &BlockLootContext<'_>,
    ) -> Option<Vec<ItemStack>> {
        self.base.get_drops(state, context)
    }
    // TODO implement set_placed_by (requires WitherSkullBlock::check_spawn to be implemented)
}

impl AbstractSkullBlock for WitherWallSkullBlock {
    fn state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.base.state_for_placement(context)
    }
}
