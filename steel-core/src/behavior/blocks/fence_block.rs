//! Fence block behavior implementation.
//!
//! Fences connect to adjacent fences, fence gates, and solid blocks.

use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, Direction};
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_block_tags::{FENCE_GATES_TAG, FENCES_TAG};
use steel_registry::vanilla_fluids;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehaviour;
use crate::behavior::context::BlockPlaceContext;
use crate::world::World;

/// Behavior for fence blocks.
///
/// Fences have 4 boolean properties (north, east, south, west) that indicate
/// whether the fence connects in that direction. A fence connects to:
/// - Other fences of the same type
/// - Fence gates facing the appropriate direction
/// - Blocks with a sturdy face on the connecting side
pub struct FenceBlock {
    block: BlockRef,
}

impl FenceBlock {
    /// North connection property.
    pub const NORTH: BoolProperty = BlockStateProperties::NORTH;
    /// East connection property.
    pub const EAST: BoolProperty = BlockStateProperties::EAST;
    /// South connection property.
    pub const SOUTH: BoolProperty = BlockStateProperties::SOUTH;
    /// West connection property.
    pub const WEST: BoolProperty = BlockStateProperties::WEST;
    /// Waterlogged property.
    pub const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;

    /// Creates a new fence block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Checks if this fence should connect to the given neighbor state.
    fn connects_to(neighbor_state: BlockStateId, direction: Direction) -> bool {
        let neighbor_block = neighbor_state.get_block();

        // Check if it's a fence (same tag)
        if REGISTRY.blocks.is_in_tag(neighbor_block, &FENCES_TAG) {
            return true;
        }

        // Check if it's a fence gate facing the right direction
        if REGISTRY.blocks.is_in_tag(neighbor_block, &FENCE_GATES_TAG) {
            // Fence gates connect perpendicular to their facing direction
            // A gate facing north/south connects to fences to its east/west
            // A gate facing east/west connects to fences to its north/south
            if let Some(gate_facing) =
                neighbor_state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING)
            {
                // Gate connects perpendicular to its facing
                let connects = match (gate_facing, direction) {
                    // Gate facing N/S connects to blocks on E/W sides,
                    // Gate facing E/W connects to blocks on N/S sides
                    (Direction::North | Direction::South, Direction::East | Direction::West)
                    | (Direction::East | Direction::West, Direction::North | Direction::South) => {
                        true
                    }
                    _ => false,
                };
                if connects {
                    return true;
                }
            }
        }

        // Check if the neighbor has a sturdy face on the opposite side
        let opposite = match direction {
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::East => Direction::West,
            Direction::West => Direction::East,
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
        };
        neighbor_state.is_face_sturdy(opposite)
    }

    /// Gets the connection state for a position by checking all 4 horizontal neighbors.
    fn get_connection_state(&self, world: &World, pos: &BlockPos) -> BlockStateId {
        let mut state = self.block.default_state();

        // Check north
        let north_pos = Direction::North.relative(pos);
        let north_state = world.get_block_state(&north_pos);
        let connects_north = Self::connects_to(north_state, Direction::North);
        state = state.set_value(&Self::NORTH, connects_north);

        // Check east
        let east_pos = Direction::East.relative(pos);
        let east_state = world.get_block_state(&east_pos);
        let connects_east = Self::connects_to(east_state, Direction::East);
        state = state.set_value(&Self::EAST, connects_east);

        // Check south
        let south_pos = Direction::South.relative(pos);
        let south_state = world.get_block_state(&south_pos);
        let connects_south = Self::connects_to(south_state, Direction::South);
        state = state.set_value(&Self::SOUTH, connects_south);

        // Check west
        let west_pos = Direction::West.relative(pos);
        let west_state = world.get_block_state(&west_pos);
        let connects_west = Self::connects_to(west_state, Direction::West);
        state = state.set_value(&Self::WEST, connects_west);

        state
    }
}

impl BlockBehaviour for FenceBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        log::debug!(
            "FenceBlock::get_state_for_placement called for {:?} at {:?}",
            self.block.key,
            context.relative_pos
        );
        Some(self.get_connection_state(context.world, &context.relative_pos))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        _world: &World,
        _pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        // Only update for horizontal directions
        match direction {
            Direction::North => {
                let connects = Self::connects_to(neighbor_state, Direction::North);
                state.set_value(&Self::NORTH, connects)
            }
            Direction::East => {
                let connects = Self::connects_to(neighbor_state, Direction::East);
                state.set_value(&Self::EAST, connects)
            }
            Direction::South => {
                let connects = Self::connects_to(neighbor_state, Direction::South);
                state.set_value(&Self::SOUTH, connects)
            }
            Direction::West => {
                let connects = Self::connects_to(neighbor_state, Direction::West);
                state.set_value(&Self::WEST, connects)
            }
            // Vertical directions don't affect fence connections
            Direction::Up | Direction::Down => state,
        }
    }

    fn get_fluid_state(&self, state: BlockStateId) -> FluidState {
        if state.get_value(&Self::WATERLOGGED) {
            FluidState::source(&vanilla_fluids::WATER)
        } else {
            FluidState::EMPTY
        }
    }
}
