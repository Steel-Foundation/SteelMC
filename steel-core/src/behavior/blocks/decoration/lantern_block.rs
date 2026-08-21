use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, BoolProperty},
        shapes::SupportType,
    },
    fluid::FluidStateExt,
    vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId, Direction, axis::Axis};

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, block::schedule_water_tick_if_waterlogged},
    entity::ai::path::PathComputationType,
    fluid::get_fluid_state,
    world::{LevelReader, ScheduledTickAccess},
};

/// Behavior for all non-weathering Lantern type blocks
#[block_behavior]
pub struct LanternBlock {
    block: BlockRef,
}

const HANGING: &BoolProperty = &BlockStateProperties::HANGING;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

impl LanternBlock {
    /// Creates a new lantern block behavior for the given block
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn get_connected_dir(state: BlockStateId) -> Direction {
        if state.get_value(HANGING) {
            return Direction::Down;
        }
        Direction::Up
    }
}

impl BlockBehavior for LanternBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let replaced_fluid_state = get_fluid_state(context.world, context.place_pos());

        for dir in context.get_nearest_looking_directions() {
            if dir.axis() == Axis::Y {
                let state = self
                    .block
                    .default_state()
                    .set_value(HANGING, dir == Direction::Up);
                if self.can_survive(state, context.world, context.place_pos()) {
                    return Some(state.set_value(
                        WATERLOGGED,
                        replaced_fluid_state.is_water() && replaced_fluid_state.is_source(),
                    ));
                }
            }
        }
        None
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let dir = Self::get_connected_dir(state).opposite();
        let relative_pos = pos.relative(dir);
        world.is_face_sturdy_for(
            world.get_block_state(relative_pos),
            relative_pos,
            dir.opposite(),
            SupportType::Center,
        )
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);

        if Self::get_connected_dir(state).opposite() == direction
            && !self.can_survive(state, world, pos)
        {
            return vanilla_blocks::AIR.default_state();
        }
        state
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}
