use std::ptr;

use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BambooLeaves, BlockStateProperties},
    },
    item_stack::ItemStack,
    vanilla_blocks, vanilla_items,
};
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::{
    behavior::{BlockBehaviour, BlockPlaceContext, blocks::crops::BambooStalkBlock},
    world::World,
};

/// Behavior for the Bamboo Sapling Block
/// /// TODO:
/// - [ ] bonemeal
/// - [ ] brightness
pub struct BambooSaplingBlock {
    block: BlockRef,
}

impl BambooSaplingBlock {
    /// Creates a new Bamboo Sapling Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Grows the Bamboo Sapling
    pub fn grow(world: &World, pos: BlockPos) {
        world.set_block(
            pos.above(),
            vanilla_blocks::BAMBOO
                .default_state()
                .set_value(&BlockStateProperties::BAMBOO_LEAVES, BambooLeaves::Small),
            UpdateFlags::UPDATE_ALL,
        );
    }
}

impl BlockBehaviour for BambooSaplingBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if !BambooStalkBlock::can_survive(world, pos) {
            return vanilla_blocks::AIR.default_state();
        }

        if direction == Direction::Up && ptr::eq(neighbor_state.get_block(), vanilla_blocks::BAMBOO)
        {
            return vanilla_blocks::BAMBOO.default_state();
        }

        state
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        true
    }

    fn random_tick(&self, _state: BlockStateId, world: &World, pos: BlockPos) {
        if rand::random_range(0..3) == 0 && world.get_block_state(&pos.above()).is_air() {
            // TODO: brightness
            Self::grow(world, pos);
        }
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(&vanilla_items::ITEMS.bamboo))
    }
}
