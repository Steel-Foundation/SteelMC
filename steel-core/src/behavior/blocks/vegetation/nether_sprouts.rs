use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    REGISTRY, TaggedRegistryExt,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_block_tags::{self},
    vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext,
        blocks::vegetation::{
            Vegetation,
            vegetation_block::{vegetation_can_survive, vegetation_update_shape},
        },
    },
    world::World,
};

/// Behavior for Nether Sprouts
#[block_behavior]
pub struct NetherSproutsBlock {
    block: BlockRef,
}

impl NetherSproutsBlock {
    /// Creates a new Nether Sprout Block Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for NetherSproutsBlock {
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
        _direction: steel_utils::Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        vegetation_update_shape(self, state, world, pos)
    }

    fn can_survive(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool {
        vegetation_can_survive(self, state, world, pos)
    }
}

impl Vegetation for NetherSproutsBlock {
    fn may_place_on(&self, state: BlockStateId, _world: &World, _pos: BlockPos) -> bool {
        state.get_block() == vanilla_blocks::SOUL_SOIL
            || REGISTRY
                .blocks
                .is_in_tag(state.get_block(), &vanilla_block_tags::NYLIUM_TAG)
            || {
                steel_registry::REGISTRY
                    .blocks
                    .is_in_tag(state.get_block(), &vanilla_block_tags::DIRT_TAG)
                    || state.get_block() == vanilla_blocks::FARMLAND
            }
    }
}
