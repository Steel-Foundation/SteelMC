use std::sync::Arc;

use steel_macros::block_behavior;
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
    behavior::{
        BlockBehavior, BlockPlaceContext,
        blocks::vegetation::{BambooStalkBlock, bonemealable::Bonemealable},
    },
    world::{LevelReader, ScheduledTickAccess, World},
};

/// Behavior for the Bamboo Sapling Block
#[block_behavior]
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
    pub fn grow(world: &Arc<World>, pos: BlockPos) {
        world.set_block(
            pos.above(),
            vanilla_blocks::BAMBOO
                .default_state()
                .set_value(&BlockStateProperties::BAMBOO_LEAVES, BambooLeaves::Small),
            UpdateFlags::UPDATE_ALL,
        );
    }
}

impl Bonemealable for BambooSaplingBlock {
    fn get_bonemeal_age_increase(&self, _world: &Arc<World>, _rng: &mut dyn rand::Rng) -> u8 {
        1
    }

    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        !world.is_outside_build_height(pos.above().y())
            && world.get_block_state(pos.above()).is_air()
    }

    fn perform_bonemeal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn rand::Rng,
        pos: BlockPos,
    ) {
        Self::grow(world, pos);
    }
}

impl BlockBehavior for BambooSaplingBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        self.can_survive(state, context.world, context.place_pos())
            .then_some(state)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if !BambooStalkBlock::can_survive(world, pos) {
            return vanilla_blocks::AIR.default_state();
        }

        if direction == Direction::Up && neighbor_state.get_block() == &vanilla_blocks::BAMBOO {
            return vanilla_blocks::BAMBOO.default_state();
        }

        state
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        BambooStalkBlock::can_survive(world, pos)
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        true
    }

    fn random_tick(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if rand::random_range(0..3) == 0
            && world.get_block_state(pos.above()).is_air()
            && world.raw_brightness(pos.above(), 0) >= 9
        {
            Self::grow(world, pos);
        }
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(&vanilla_items::BAMBOO))
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
