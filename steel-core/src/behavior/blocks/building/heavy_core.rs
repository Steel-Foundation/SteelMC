use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, block::schedule_water_tick_if_waterlogged},
    entity::ai::path::PathComputationType,
    fluid::{FluidStateExt as _, get_fluid_state},
    world::ScheduledTickAccess,
};
use steel_macros::block_behavior;
use steel_registry::blocks::{
    BlockRef,
    block_state_ext::BlockStateExt,
    properties::{BlockStateProperties, BoolProperty},
};
use steel_utils::{BlockPos, BlockStateId, Direction};

/// Behavior for vanilla budding amethyst blocks.
#[block_behavior]
pub struct HeavyCoreBlock {
    block: BlockRef,
}

const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

impl HeavyCoreBlock {
    /// Creates a new budding amethyst block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for HeavyCoreBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);

        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let replaced_fluid_state = get_fluid_state(context.world, context.place_pos());
        let is_water_source = replaced_fluid_state.is_water() && replaced_fluid_state.is_source();
        Some(
            self.block
                .default_state()
                .set_value(WATERLOGGED, is_water_source),
        )
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}
