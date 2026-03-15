use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    TaggedRegistryExt,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_block_tags, vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, BlockStateBehaviorExt,
        blocks::vegetation::{
            Vegetation,
            bonemealable::Bonemealable,
            vegetation_block::{vegetation_can_survive, vegetation_update_shape},
        },
    },
    world::World,
};

/// Behavior for the Azalea Block
#[block_behavior]
pub struct AzaleaBlock {
    block: BlockRef,
}

impl AzaleaBlock {
    /// Creates a new Azalea Block Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for AzaleaBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if self.may_place_on(
            context.world.get_block_state(context.relative_pos.below()),
            context.world,
            context.relative_pos.below(),
        ) {
            Some(self.block.default_state())
        } else {
            None
        }
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        vegetation_update_shape(self, state, world, pos)
    }

    fn can_survive(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool {
        vegetation_can_survive(self, state, world, pos)
    }
}

impl Vegetation for AzaleaBlock {
    fn may_place_on(&self, state: BlockStateId, _world: &World, _pos: BlockPos) -> bool {
        state.get_block() == vanilla_blocks::CLAY
            || steel_registry::REGISTRY
                .blocks
                .is_in_tag(state.get_block(), &vanilla_block_tags::DIRT_TAG)
            || state.get_block() == vanilla_blocks::FARMLAND
    }
}

impl Bonemealable for AzaleaBlock {
    fn is_bonemealable(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool {
        world
            .get_block_state(pos.above())
            .get_fluid_state()
            .is_empty()
    }

    fn random_success(&self) -> bool {
        rand::random_bool(0.45f64)
    }

    fn apply_bonemeal(&self, _state: BlockStateId, _world: &Arc<World>, _pos: BlockPos) {
        // TODO: grow tree
    }
}
