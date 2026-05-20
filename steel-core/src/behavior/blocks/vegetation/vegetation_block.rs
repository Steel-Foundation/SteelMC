use steel_registry::{
    REGISTRY, TaggedRegistryExt,
    blocks::{
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, DoubleBlockHalf},
    },
    vanilla_block_tags, vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId, Direction, math::Axis};

use crate::{
    behavior::BlockBehavior,
    world::{LevelReader, ScheduledTickAccess},
};

/// Common behavior for vegetation blocks
pub trait Vegetation {
    /// Checks if the vegetation block can be placed on the given block state below on the given position below.
    fn may_place_on(&self, state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        REGISTRY.blocks.is_in_tag(
            state.get_block(),
            &vanilla_block_tags::SUPPORTS_VEGETATION_TAG,
        )
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

/// Shared update-shape logic for vegetation.
///
/// Important: this calls the final `BlockBehavior::can_survive`,
/// not `vegetation_can_survive`, so leaf blocks can override survival.
pub fn vegetation_update_shape<B: BlockBehavior>(
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

/// Shared survival logic for double plants.
pub fn double_plant_can_survive<H: Vegetation>(
    hooks: &H,
    state: BlockStateId,
    world: &dyn LevelReader,
    pos: BlockPos,
) -> bool {
    if state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Upper {
        let state_below = world.get_block_state(pos.below());
        state_below.get_block() == state.get_block()
            && state_below.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF)
                == DoubleBlockHalf::Lower
    } else {
        vegetation_can_survive(hooks, state, world, pos)
    }
}

/// Shared update-shape logic for double plants.
///
/// This mirrors the Java superclass logic, but explicitly.
pub fn double_plant_update_shape<B: BlockBehavior>(
    block: &B,
    state: BlockStateId,
    world: &dyn ScheduledTickAccess,
    pos: BlockPos,
    direction: Direction,
    neighbor_state: BlockStateId,
) -> BlockStateId {
    let half = state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF);

    if direction.axis() != Axis::Y
        || ((half == DoubleBlockHalf::Lower) != (direction == Direction::Up))
        || (neighbor_state.get_block() == state.get_block()
            && neighbor_state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) != half)
    {
        if half == DoubleBlockHalf::Lower
            && direction == Direction::Down
            && !block.can_survive(state, world, pos)
        {
            return vanilla_blocks::AIR.default_state();
        }

        vegetation_update_shape(block, state, world, pos)
    } else {
        vanilla_blocks::AIR.default_state()
    }
}
