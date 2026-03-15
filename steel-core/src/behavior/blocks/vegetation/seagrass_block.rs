use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, DoubleBlockHalf},
    },
    fluid::{FluidRef, FluidState, FluidStateExt},
    vanilla_blocks, vanilla_fluids,
};
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::{
    behavior::{
        BlockBehaviour, BlockPlaceContext, BlockStateBehaviorExt,
        blocks::vegetation::{
            Vegetation, bonemealable::Bonemealable, vegetation_block::vegetation_update_shape,
        },
    },
    world::World,
};

/// Behavior for the Seagrass Block
#[block_behavior]
pub struct SeagrassBlock {
    block: BlockRef,
}

impl SeagrassBlock {
    /// Creates a new Seagrass Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for SeagrassBlock {
    fn get_state_for_placement(
        &self,
        context: &BlockPlaceContext<'_>,
    ) -> Option<steel_utils::BlockStateId> {
        let fluid_state = context
            .world
            .get_block_state(&context.relative_pos)
            .get_fluid_state();
        if fluid_state.is_water() && fluid_state.amount == 8 {
            return Some(self.block.default_state());
        }

        None
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let new_state = vegetation_update_shape(self, state, world, pos);
        if !new_state.is_air() {
            world.schedule_fluid_tick_default(
                pos,
                &vanilla_fluids::WATER,
                vanilla_fluids::WATER.tick_delay as i32,
            );
        }
        new_state
    }

    fn can_survive(&self, _state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        let state_below = world.get_block_state(&pos.below());
        self.may_place_on(state_below, world, pos.below())
    }

    fn get_fluid_state(&self, _state: BlockStateId) -> FluidState {
        FluidState::source(&vanilla_fluids::WATER)
    }

    fn place_liquid(
        &self,
        _world: &World,
        _pos: BlockPos,
        _state: BlockStateId,
        _fluid_state: FluidState,
    ) -> bool {
        false
    }

    fn can_place_liquid(&self, _state: BlockStateId, _fluid: FluidRef) -> bool {
        false
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for SeagrassBlock {
    fn is_bonemealable(&self, _state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        world.get_block_state(&pos.above()).get_block() == vanilla_blocks::WATER
    }

    fn apply_bonemeal(&self, _state: BlockStateId, world: &World, pos: BlockPos) {
        let lower_state = vanilla_blocks::TALL_SEAGRASS.default_state();
        let upper_state = lower_state.set_value(
            &BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Upper,
        );
        let pos_above = pos.above();
        world.set_block(pos, lower_state, UpdateFlags::UPDATE_CLIENTS);
        world.set_block(pos_above, upper_state, UpdateFlags::UPDATE_CLIENTS);
    }
}

impl Vegetation for SeagrassBlock {
    fn may_place_on(&self, state: BlockStateId, _world: &World, _pos: BlockPos) -> bool {
        state.is_face_sturdy(Direction::Up) && state.get_block() != vanilla_blocks::MAGMA_BLOCK
    }
}
