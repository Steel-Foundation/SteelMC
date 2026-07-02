//! Chain block behavior implementation.
//!
//! Chains are oriented blocks with an axis property that determines their direction.
//! They can also be waterlogged.

use crate::behavior::blocks::{WeatherState, WeatheringCopper};
use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::entity::ai::path::PathComputationType;
use crate::world::{ScheduledTickAccess, World};
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, EnumProperty,
};
use steel_registry::vanilla_fluids;
use steel_utils::axis::Axis;
use steel_utils::{BlockPos, BlockStateId};

/// Behavior for chain blocks (iron chain, waxed copper chains).
///
/// Chains have an axis property that is set based on which face was clicked
/// during placement, and can be waterlogged.
#[block_behavior]
pub struct ChainBlock {
    block: BlockRef,
}

/// Axis property for the chain orientation.
const AXIS: EnumProperty<Axis> = BlockStateProperties::AXIS;
/// Waterlogged property.
const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;

impl ChainBlock {
    /// Creates a new chain block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for ChainBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(&AXIS, context.clicked_face.get_axis())
                .set_value(&WATERLOGGED, context.is_water_source()),
        )
    }

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

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}
/// Behavior for weathering copper chain blocks.
///
/// Copper chains have an axis property that is set based on which face was clicked
/// during placement, can be waterlogged, and will oxidize over time.
#[block_behavior]
pub struct WeatheringCopperChainBlock {
    block: BlockRef,
    #[json_arg(r#enum = "WeatherState", json = "weather_state")]
    weathering: WeatheringCopper,
}

impl WeatheringCopperChainBlock {
    /// Creates a new weathering copper chain block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef, weather_state: WeatherState) -> Self {
        Self {
            block,
            weathering: WeatheringCopper::new(weather_state),
        }
    }
}

impl BlockBehavior for WeatheringCopperChainBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(&AXIS, context.clicked_face.get_axis())
                .set_value(&WATERLOGGED, context.is_water_source()),
        )
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        self.weathering.is_randomly_ticking()
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.weathering.change_over_time(state, world, pos);
    }

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

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}

fn schedule_water_tick_if_waterlogged(
    state: BlockStateId,
    world: &dyn ScheduledTickAccess,
    pos: BlockPos,
) {
    if state.get_value(&WATERLOGGED) {
        let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
        world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
    }
}
