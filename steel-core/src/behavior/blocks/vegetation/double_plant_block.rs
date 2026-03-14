use steel_registry::blocks::{
    BlockRef,
    block_state_ext::BlockStateExt,
    properties::{self, BlockStateProperties},
};
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::{
    behavior::{
        BlockBehaviour, BlockPlaceContext,
        blocks::vegetation::{
            Vegetation,
            vegetation_block::{double_plant_can_survive, double_plant_update_shape},
        },
    },
    world::World,
};

/// Behavior for Two High Plants
pub struct DoublePlantBlock {
    block: BlockRef,
}

impl DoublePlantBlock {
    /// Creates a new Double Plant Block Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl Vegetation for DoublePlantBlock {}

impl BlockBehaviour for DoublePlantBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if context.relative_pos.y() < context.world.get_max_y()
            && context
                .world
                .get_block_state(&context.relative_pos.above())
                .is_replaceable()
        {
            Some(self.block.default_state())
        } else {
            None
        }
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        // FIXME: dont know if this is correct
        let waterlogged_state = state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .map_or(state, |waterlogged| {
                state.set_value(&BlockStateProperties::WATERLOGGED, waterlogged)
            });
        world.set_block(
            pos.above(),
            waterlogged_state.set_value(&BlockStateProperties::HALF, properties::Half::Top),
            UpdateFlags::UPDATE_ALL,
        );
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
        double_plant_update_shape(self, state, world, pos, direction, neighbor_state)
    }

    fn can_survive(&self, state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        double_plant_can_survive(self, state, world, pos)
    }
}
