//! Wall block behavior implementation.
//!
//! Walls connect to adjacent walls, bars, fence gates and solid blocks. Each
//! horizontal side has a [`WallSide`] (none/low/tall) and an `UP` post flag.

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, EnumProperty, WallSide,
};
use steel_registry::blocks::shapes::{OffsetVoxelShape, offset_face_rectangles_cover};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_fluids;
use steel_registry::vanilla_fluids::WATER;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::building::FenceGateBlock;
use crate::behavior::blocks::utils::is_excluded_for_connection;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::ai::path::PathComputationType;
use crate::world::ScheduledTickAccess;

/// Behavior for wall blocks.
///
/// Walls have four [`WallSide`] properties (north, east, south, west) plus an
/// `UP` post flag and a `WATERLOGGED` flag. A wall connects to:
/// - Other walls
/// - Iron bars
/// - Fence gates facing the appropriate direction
/// - Blocks with a sturdy face on the connecting side
#[block_behavior]
pub struct WallBlock {
    block: BlockRef,
}

/// Post (center column) property.
const UP: BoolProperty = BlockStateProperties::UP;
/// North connection property.
const NORTH: EnumProperty<WallSide> = BlockStateProperties::NORTH_WALL;
/// East connection property.
const EAST: EnumProperty<WallSide> = BlockStateProperties::EAST_WALL;
/// South connection property.
const SOUTH: EnumProperty<WallSide> = BlockStateProperties::SOUTH_WALL;
/// West connection property.
const WEST: EnumProperty<WallSide> = BlockStateProperties::WEST_WALL;
/// Waterlogged property.
const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;

// Vanilla TEST_SHAPE_POST = Block.column(2.0, 0.0, 16.0), projected onto the DOWN face.
const POST_X_MIN: f64 = 7.0 / 16.0;
const POST_X_MAX: f64 = 9.0 / 16.0;
const POST_Z_MIN: f64 = 7.0 / 16.0;
const POST_Z_MAX: f64 = 9.0 / 16.0;

// Vanilla TEST_SHAPES_WALL = Shapes.rotateHorizontal(Block.boxZ(2.0, 16.0, 0.0, 9.0)),
// projected onto the DOWN face per direction.
const WALL_ARM_MIN: f64 = 7.0 / 16.0;
const WALL_ARM_MAX: f64 = 9.0 / 16.0;
const WALL_ARM_EXTENT: f64 = 9.0 / 16.0;

impl WallBlock {
    /// Creates a new wall block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for WallBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let world = context.world;
        let pos = context.relative_pos;

        let north_pos = Direction::North.relative(pos);
        let east_pos = Direction::East.relative(pos);
        let south_pos = Direction::South.relative(pos);
        let west_pos = Direction::West.relative(pos);
        let top_pos = Direction::Up.relative(pos);

        let north_state = world.get_block_state(north_pos);
        let east_state = world.get_block_state(east_pos);
        let south_state = world.get_block_state(south_pos);
        let west_state = world.get_block_state(west_pos);
        let top_state = world.get_block_state(top_pos);

        // Vanilla checks the neighbor's face that points back at the wall,
        // i.e. the opposite of the direction toward the neighbor.
        let north = connects_to(
            north_state,
            north_state.is_face_sturdy_at(north_pos, Direction::South),
            Direction::South,
        );
        let east = connects_to(
            east_state,
            east_state.is_face_sturdy_at(east_pos, Direction::West),
            Direction::West,
        );
        let south = connects_to(
            south_state,
            south_state.is_face_sturdy_at(south_pos, Direction::North),
            Direction::North,
        );
        let west = connects_to(
            west_state,
            west_state.is_face_sturdy_at(west_pos, Direction::East),
            Direction::East,
        );

        let state = self
            .block
            .default_state()
            .set_value(&WATERLOGGED, context.is_water_source());

        Some(update_wall_state(
            state, top_pos, top_state, north, east, south, west,
        ))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if state.get_value(&WATERLOGGED) {
            let water = &vanilla_fluids::WATER;
            world.schedule_fluid_tick_default(pos, water, world.fluid_tick_delay(&WATER));
        }

        match direction {
            // Base behavior: nothing below changes a wall's shape.
            Direction::Down => state,
            Direction::Up => top_update(state, neighbor_pos, neighbor_state),
            _ => side_update(world, pos, state, neighbor_pos, neighbor_state, direction),
        }
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}

/// Whether the wall is connected on the given side.
fn is_connected(state: BlockStateId, side: &EnumProperty<WallSide>) -> bool {
    state.get_value(side) != WallSide::None
}

/// Vanilla `WallBlock.connectsTo`.
///
/// `face_solid` is whether the neighbor has a sturdy face pointing back at
/// the wall; `direction` is that same (opposite-to-neighbor) direction.
fn connects_to(neighbor_state: BlockStateId, face_solid: bool, direction: Direction) -> bool {
    let block = neighbor_state.get_block();
    let connected_fence_gate = block.has_tag(&BlockTag::FENCE_GATES)
        && FenceGateBlock::connects_to_direction(neighbor_state, direction);

    block.has_tag(&BlockTag::WALLS)
        || (!is_excluded_for_connection(block) && face_solid)
        || block.has_tag(&BlockTag::BARS)
        || block.has_tag(&BlockTag::C_GLASS_PANES)
        || connected_fence_gate
}

/// Vanilla `WallBlock.topUpdate`.
fn top_update(state: BlockStateId, top_pos: BlockPos, top_neighbor: BlockStateId) -> BlockStateId {
    let north = is_connected(state, &NORTH);
    let east = is_connected(state, &EAST);
    let south = is_connected(state, &SOUTH);
    let west = is_connected(state, &WEST);
    update_wall_state(state, top_pos, top_neighbor, north, east, south, west)
}

/// Vanilla `WallBlock.sideUpdate`.
fn side_update(
    world: &dyn ScheduledTickAccess,
    pos: BlockPos,
    state: BlockStateId,
    neighbor_pos: BlockPos,
    neighbor: BlockStateId,
    direction: Direction,
) -> BlockStateId {
    let opposite = direction.opposite();
    let connected = connects_to(
        neighbor,
        neighbor.is_face_sturdy_at(neighbor_pos, opposite),
        opposite,
    );

    let north = if direction == Direction::North {
        connected
    } else {
        is_connected(state, &NORTH)
    };
    let east = if direction == Direction::East {
        connected
    } else {
        is_connected(state, &EAST)
    };
    let south = if direction == Direction::South {
        connected
    } else {
        is_connected(state, &SOUTH)
    };
    let west = if direction == Direction::West {
        connected
    } else {
        is_connected(state, &WEST)
    };

    let above = Direction::Up.relative(pos);
    let above_state = world.get_block_state(above);
    update_wall_state(state, above, above_state, north, east, south, west)
}

/// Vanilla `WallBlock.updateShape` (private side/post helper).
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "mirrors vanilla WallBlock north/east/south/west signature"
)]
fn update_wall_state(
    state: BlockStateId,
    top_pos: BlockPos,
    top_neighbor: BlockStateId,
    north: bool,
    east: bool,
    south: bool,
    west: bool,
) -> BlockStateId {
    let above_shape = top_neighbor.get_collision_shape_at(top_pos);
    let sides = update_sides(state, above_shape, north, east, south, west);
    sides.set_value(&UP, should_raise_post(sides, top_neighbor, above_shape))
}

/// Vanilla `WallBlock.updateSides`.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "mirrors vanilla WallBlock north/east/south/west signature"
)]
fn update_sides(
    state: BlockStateId,
    above_shape: OffsetVoxelShape,
    north: bool,
    east: bool,
    south: bool,
    west: bool,
) -> BlockStateId {
    state
        .set_value(
            &NORTH,
            make_wall_state(
                north,
                above_shape,
                WALL_ARM_MIN,
                WALL_ARM_MAX,
                0.0,
                WALL_ARM_EXTENT,
            ),
        )
        .set_value(
            &EAST,
            make_wall_state(
                east,
                above_shape,
                WALL_ARM_MIN,
                1.0,
                WALL_ARM_MIN,
                WALL_ARM_MAX,
            ),
        )
        .set_value(
            &SOUTH,
            make_wall_state(
                south,
                above_shape,
                WALL_ARM_MIN,
                WALL_ARM_MAX,
                WALL_ARM_MIN,
                1.0,
            ),
        )
        .set_value(
            &WEST,
            make_wall_state(
                west,
                above_shape,
                0.0,
                WALL_ARM_EXTENT,
                WALL_ARM_MIN,
                WALL_ARM_MAX,
            ),
        )
}

/// Vanilla `WallBlock.makeWallState`.
fn make_wall_state(
    connects_to_side: bool,
    above_shape: OffsetVoxelShape,
    x_min: f64,
    x_max: f64,
    z_min: f64,
    z_max: f64,
) -> WallSide {
    if !connects_to_side {
        return WallSide::None;
    }
    if is_covered(above_shape, x_min, x_max, z_min, z_max) {
        WallSide::Tall
    } else {
        WallSide::Low
    }
}

/// Vanilla `WallBlock.shouldRaisePost`.
fn should_raise_post(
    state: BlockStateId,
    top_neighbor: BlockStateId,
    above_shape: OffsetVoxelShape,
) -> bool {
    let top_neighbor_has_post = top_neighbor.get_block().has_tag(&BlockTag::WALLS)
        && top_neighbor.try_get_value(&UP).unwrap_or(false);
    if top_neighbor_has_post {
        return true;
    }

    let north_wall = state.get_value(&NORTH);
    let south_wall = state.get_value(&SOUTH);
    let east_wall = state.get_value(&EAST);
    let west_wall = state.get_value(&WEST);

    let north_none = north_wall == WallSide::None;
    let south_none = south_wall == WallSide::None;
    let east_none = east_wall == WallSide::None;
    let west_none = west_wall == WallSide::None;

    let has_corner = (north_none && south_none && west_none && east_none)
        || (north_none != south_none)
        || (west_none != east_none);
    if has_corner {
        return true;
    }

    let has_high_wall = (north_wall == WallSide::Tall && south_wall == WallSide::Tall)
        || (east_wall == WallSide::Tall && west_wall == WallSide::Tall);
    if has_high_wall {
        return false;
    }

    top_neighbor
        .get_block()
        .has_tag(&BlockTag::WALL_POST_OVERRIDE)
        || is_covered(above_shape, POST_X_MIN, POST_X_MAX, POST_Z_MIN, POST_Z_MAX)
}

/// Vanilla `WallBlock.isCovered`.
///
/// Checks whether the block above's collision shape fully covers a test
/// rectangle on its DOWN face.
fn is_covered(
    above_shape: OffsetVoxelShape,
    x_min: f64,
    x_max: f64,
    z_min: f64,
    z_max: f64,
) -> bool {
    offset_face_rectangles_cover(above_shape, Direction::Down, x_min, x_max, z_min, z_max)
}
