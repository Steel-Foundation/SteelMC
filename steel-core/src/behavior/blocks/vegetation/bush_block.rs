use rand::Rng;
use steel_macros::block_behavior;
use steel_registry::vanilla_blocks;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use super::{BlockRef, default_surviving_state};
use crate::behavior::blocks::vegetation::Vegetation;
use crate::behavior::{
    block::BlockBehavior, blocks::vegetation::vegetation_block::vegetation_can_survive,
};
use crate::behavior::{
    blocks::vegetation::bonemealable::{
        Bonemealable, find_spreadable_neighbor_pos, has_spreadable_neighbor_pos,
    },
    context::BlockPlaceContext,
};
use crate::world::{LevelReader, ScheduledTickAccess, World};
use std::sync::Arc;

/// Vanilla `BushBlock` survival.
#[block_behavior]
pub struct BushBlock {
    block: BlockRef,
}

impl BushBlock {
    /// Creates a new bush block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for BushBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        vegetation_can_survive(self, state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
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
        if !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }
        state
    }
    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
impl Vegetation for BushBlock {}
impl Bonemealable for BushBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        has_spreadable_neighbor_pos(world, pos, state)
    }
    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        let Some(block_pos) = find_spreadable_neighbor_pos(world, pos, state) else {
            return;
        };
        world.set_block(
            block_pos,
            self.block.default_state(),
            UpdateFlags::UPDATE_ALL,
        );
    }
}
