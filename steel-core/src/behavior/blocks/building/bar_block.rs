//! bar block behavior implementation.
//!
//! bars connect to adjacent bars, bar solid blocks.

use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, Direction};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::WeatherState;
use crate::behavior::blocks::building::WeatheringCopper;
use crate::behavior::blocks::utils::is_excluded_for_connection;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::ai::path::PathComputationType;
use crate::world::{ScheduledTickAccess, World};
use steel_registry::vanilla_fluids;

/// Behavior for bar blocks.
///
/// bars have 4 boolean properties (north, east, south, west) that indicate
/// whether the bar connects in that direction. A bar connects to:
/// - Other bars of the same type
/// - bar gates facing the appropriate direction
/// - Blocks with a sturdy face on the connecting side
#[block_behavior]
pub struct IronBarsBlock {
    block: BlockRef,
}

/// North connection property.
const NORTH: BoolProperty = BlockStateProperties::NORTH;
/// East connection property.
const EAST: BoolProperty = BlockStateProperties::EAST;
/// South connection property.
const SOUTH: BoolProperty = BlockStateProperties::SOUTH;
/// West connection property.
const WEST: BoolProperty = BlockStateProperties::WEST;
/// Waterlogged property.
const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;

impl IronBarsBlock {
    /// Creates a new bar block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for IronBarsBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);
        update_shape(state, neighbor_state, neighbor_pos, direction)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            get_connection_state(self.block, context.world, &context.relative_pos)
                .set_value(&WATERLOGGED, context.is_water_source()),
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

/// Behavior for copper bar blocks.
///
/// bars have 4 boolean properties (north, east, south, west) that indicate
/// whether the bar connects in that direction. A bar connects to:
/// - Other bars of the same type
/// - bar gates facing the appropriate direction
/// - Blocks with a sturdy face on the connecting side
#[block_behavior]
pub struct WeatheringCopperBarsBlock {
    block: BlockRef,
    #[json_arg(r#enum = "WeatherState", json = "weather_state")]
    weathering: WeatheringCopper,
}

impl WeatheringCopperBarsBlock {
    /// Creates a new bar block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef, weather_state: WeatherState) -> Self {
        Self {
            block,
            weathering: WeatheringCopper::new(weather_state),
        }
    }
}

impl BlockBehavior for WeatheringCopperBarsBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);
        update_shape(state, neighbor_state, neighbor_pos, direction)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            get_connection_state(self.block, context.world, &context.relative_pos)
                .set_value(&WATERLOGGED, context.is_water_source()),
        )
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        self.weathering.is_randomly_ticking()
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.weathering.change_over_time(state, world, pos);
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

/// Checks if this bar should connect to the given neighbor state.
fn connects_to(neighbor_state: BlockStateId, neighbor_pos: BlockPos, direction: Direction) -> bool {
    let neighbor_block = neighbor_state.get_block();
    let excluded = is_excluded_for_connection(neighbor_block);
    (!excluded && neighbor_state.is_face_sturdy_at(neighbor_pos, direction.opposite()))
        || neighbor_block.has_tag(&BlockTag::BARS)
        || neighbor_block.has_tag(&BlockTag::WALLS)
        || neighbor_block.has_tag(&BlockTag::C_GLASS_PANES)
}

/// Gets the connection state for a position by checking all 4 horizontal neighbors.
pub fn get_connection_state(block: BlockRef, world: &World, pos: &BlockPos) -> BlockStateId {
    let mut state = block.default_state();

    // Check north
    let north_pos = Direction::North.relative(*pos);
    let north_state = world.get_block_state(north_pos);
    let connects_north = connects_to(north_state, north_pos, Direction::North);
    state = state.set_value(&NORTH, connects_north);

    // Check east
    let east_pos = Direction::East.relative(*pos);
    let east_state = world.get_block_state(east_pos);
    let connects_east = connects_to(east_state, east_pos, Direction::East);
    state = state.set_value(&EAST, connects_east);

    // Check south
    let south_pos = Direction::South.relative(*pos);
    let south_state = world.get_block_state(south_pos);
    let connects_south = connects_to(south_state, south_pos, Direction::South);
    state = state.set_value(&SOUTH, connects_south);

    // Check west
    let west_pos = Direction::West.relative(*pos);
    let west_state = world.get_block_state(west_pos);
    let connects_west = connects_to(west_state, west_pos, Direction::West);
    state = state.set_value(&WEST, connects_west);

    state
}

pub fn update_shape(
    state: BlockStateId,
    neighbor_state: BlockStateId,
    neighbor_pos: BlockPos,
    direction: Direction,
) -> BlockStateId {
    match direction {
        Direction::North => {
            let connects = connects_to(neighbor_state, neighbor_pos, Direction::North);
            state.set_value(&NORTH, connects)
        }
        Direction::East => {
            let connects = connects_to(neighbor_state, neighbor_pos, Direction::East);
            state.set_value(&EAST, connects)
        }
        Direction::South => {
            let connects = connects_to(neighbor_state, neighbor_pos, Direction::South);
            state.set_value(&SOUTH, connects)
        }
        Direction::West => {
            let connects = connects_to(neighbor_state, neighbor_pos, Direction::West);
            state.set_value(&WEST, connects)
        }
        Direction::Up | Direction::Down => state,
    }
}
