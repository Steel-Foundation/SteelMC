use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, Half},
    },
    fluid::{FluidRef, FluidState, FluidStateExt},
    item_stack::ItemStack,
    vanilla_blocks, vanilla_fluids, vanilla_items,
};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::{
    behavior::{
        BlockBehaviour, BlockPlaceContext, BlockStateBehaviorExt,
        blocks::crops::{Vegetation, vegetation_block::DoublePlant},
    },
    world::World,
};

/// Behavior for Tall Seagrass
pub struct TallSeagrassBlock {
    block: BlockRef,
}

impl TallSeagrassBlock {
    /// Creates a new Tall Seagrass Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for TallSeagrassBlock {
    fn get_state_for_placement(
        &self,
        context: &BlockPlaceContext<'_>,
    ) -> Option<steel_utils::BlockStateId> {
        if context.relative_pos.y() < context.world.get_max_y() {
            let state_above = context.world.get_block_state(&context.relative_pos.above());
            let fluid_state_above = state_above.get_fluid_state();
            if fluid_state_above.is_water() && fluid_state_above.amount == 8 {
                return Some(self.block.default_state());
            }
        }
        None
    }

    fn can_survive(&self, state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        self.double_plant_can_survive(state, world, pos)
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
        self.double_plant_update_shape(state, world, pos, direction, neighbor_state)
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: steel_utils::BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(&vanilla_items::ITEMS.seagrass))
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
}

impl DoublePlant for TallSeagrassBlock {
    fn double_plant_can_survive(&self, state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        let state_below = world.get_block_state(&pos.below());
        let half = state.get_value(&BlockStateProperties::HALF);
        if half == Half::Top {
            state_below.get_block() == self.block
                && state_below.get_value(&BlockStateProperties::HALF) == Half::Bottom
        } else {
            let fluid_state = world.get_block_state(&pos).get_fluid_state();
            self.vegetation_can_survive(state, world, pos)
                && fluid_state.is_water()
                && fluid_state.amount == 8
        }
    }
}

impl Vegetation for TallSeagrassBlock {
    fn may_place_on(&self, state: BlockStateId, _world: &World, _pos: BlockPos) -> bool {
        state.is_face_sturdy(Direction::Up) && state.get_block() != vanilla_blocks::MAGMA_BLOCK
    }
}
