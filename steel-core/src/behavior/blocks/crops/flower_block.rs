use steel_registry::blocks::BlockRef;
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    behavior::{BlockBehaviour, BlockPlaceContext, blocks::crops::Vegetation},
    world::World,
};

/// Behavior for all most flower blocks
pub struct FlowerBlock {
    block: BlockRef,
}

impl FlowerBlock {
    /// Creates a new Flower Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for FlowerBlock {
    fn get_state_for_placement(
        &self,
        context: &BlockPlaceContext<'_>,
    ) -> Option<steel_utils::BlockStateId> {
        if self.may_place_on(
            context.world.get_block_state(&context.relative_pos.below()),
            context.world,
            context.relative_pos.below(),
        ) {
            Some(self.block.default_state())
        } else {
            None
        }
    }

    fn can_survive(&self, state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        self.vegetation_can_survive(state, world, pos)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _direction: steel_utils::Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.vegetation_update_shape(state, world, pos)
    }
}

impl Vegetation for FlowerBlock {
    fn can_survive_dispatch(&self, state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        self.vegetation_can_survive(state, world, pos)
    }
}
