use steel_registry::{
    blocks::block_state_ext::BlockStateExt, vanilla_block_tags::BlockTag, vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    behavior::BlockBehavior,
    world::{LevelReader, ScheduledTickAccess},
};

/// Common behavior for vegetation blocks
pub trait Vegetation {
    /// Checks if the vegetation block can be placed on the given block state below on the given position below.
    fn may_place_on(&self, state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        state.get_block().has_tag(&BlockTag::SUPPORTS_VEGETATION)
    }
}

/// Shared survival logic for basic vegetation.
pub fn vegetation_can_survive<H: Vegetation>(
    hooks: &H,
    _state: BlockStateId,
    world: &dyn LevelReader,
    pos: BlockPos,
) -> bool {
    let state_below = world.get_block_state(pos.below());
    hooks.may_place_on(state_below, world, pos.below())
}

/// Shared update-shape logic for blocks that break when they can no longer survive.
///
/// Important: this calls the final `BlockBehavior::can_survive`,
/// not `vegetation_can_survive`, so leaf blocks can override survival.
pub fn survival_update_shape<B: BlockBehavior>(
    block: &B,
    state: BlockStateId,
    world: &dyn ScheduledTickAccess,
    pos: BlockPos,
) -> BlockStateId {
    if block.can_survive(state, world, pos) {
        state
    } else {
        vanilla_blocks::AIR.default_state()
    }
}
