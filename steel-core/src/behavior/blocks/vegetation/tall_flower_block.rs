use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::{
    BlockRef,
    block_state_ext::BlockStateExt,
    properties::{BlockStateProperties, Half},
};
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext,
        blocks::vegetation::{
            Vegetation,
            bonemealable::Bonemealable,
            vegetation_block::{double_plant_can_survive, double_plant_update_shape},
        },
    },
    world::World,
};

/// Behavior for Two High Flowers
#[block_behavior]
pub struct TallFlowerBlock {
    block: BlockRef,
}

impl TallFlowerBlock {
    /// Creates a new Tall Flower Block Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl Vegetation for TallFlowerBlock {}

impl BlockBehavior for TallFlowerBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if context.relative_pos.y() < context.world.get_max_y()
            && context
                .world
                .get_block_state(context.relative_pos.above())
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
        world: &Arc<World>,
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
            waterlogged_state.set_value(&BlockStateProperties::HALF, Half::Top),
            UpdateFlags::UPDATE_ALL,
        );
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        double_plant_update_shape(self, state, world, pos, direction, neighbor_state)
    }

    fn can_survive(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool {
        double_plant_can_survive(self, state, world, pos)
    }
}

impl Bonemealable for TallFlowerBlock {
    fn is_bonemealable(&self, _state: BlockStateId, _world: &Arc<World>, _pos: BlockPos) -> bool {
        true
    }

    fn apply_bonemeal(&self, _state: BlockStateId, _world: &Arc<World>, _pos: BlockPos) {
        // FIXME: pop_resource only works on a &Arc<World>
    }
}
