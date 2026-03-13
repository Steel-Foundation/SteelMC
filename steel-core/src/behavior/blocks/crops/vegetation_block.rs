use steel_registry::{
    REGISTRY,
    blocks::{
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, Half},
    },
    vanilla_block_tags, vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId, Direction, math::Axis};

use crate::world::World;

/// Common behavior for vegetation blocks
pub trait Vegetation {
    /// Checks if the vegetation block can be placed on the given block state below on the given position below.
    fn may_place_on(&self, state: BlockStateId, _world: &World, _pos: BlockPos) -> bool {
        REGISTRY
            .blocks
            .is_in_tag(state.get_block(), &vanilla_block_tags::DIRT_TAG)
            || state.get_block() == vanilla_blocks::FARMLAND
    }

    #[expect(unused_variables)]
    /// Returns whether the vegetation block can survive at the given position
    fn vegetation_can_survive(&self, state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        let state_below = world.get_block_state(&pos.below());
        self.may_place_on(state_below, world, pos.below())
    }

    /// Updates the shape of the block
    fn vegetation_update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
    ) -> BlockStateId {
        let state_below = world.get_block_state(&pos.below());
        if self.vegetation_can_survive(state_below, world, pos.below()) {
            state
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }
}

pub trait DoublePlant: Vegetation {
    fn double_plant_can_survive(&self, state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        if state.get_value(&BlockStateProperties::HALF) == Half::Top {
            let state_below = world.get_block_state(&pos.below());
            state_below.get_block() == state.get_block()
                && state_below.get_value(&BlockStateProperties::HALF) == Half::Bottom
        } else {
            self.vegetation_can_survive(state, world, pos)
        }
    }

    fn double_plant_update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        direction: Direction,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let half = state.get_value(&BlockStateProperties::HALF);

        if direction.axis() != Axis::Y
            || ((half == Half::Bottom) != (direction == Direction::Up))
            || (neighbor_state.get_block() == state.get_block()
                && neighbor_state.get_value(&BlockStateProperties::HALF) != half)
        {
            if half == Half::Bottom
                && direction == Direction::Down
                && !self.double_plant_can_survive(state, world, pos)
            {
                return vanilla_blocks::AIR.default_state();
            }

            self.vegetation_update_shape(state, world, pos)
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }
}
