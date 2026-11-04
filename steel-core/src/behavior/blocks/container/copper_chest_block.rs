//! Copper chest behavior
// TODO: Create the chest block entity with inventory loot table lock and custom name support
// TODO: Combine connected halves into one container/ menu and check blocks and sitting cats above them
// TODO: Open the chest menu award OPEN_CHEST and anger nearby piglins on interaction
// TODO: Track viewers recheck them on scheduled ticks and synchronize opening through block events
// TODO: Use the extracted open or close sounds for each copper chest variant
// TODO: Expose container fullness to comparators and update neighbors after removal
// TODO: Reject pathfinding through copper chests for every path computation type
// TODO: Support facing rotation and mirroring for structure transforms
// TODO: Provide copperblock conversion with connected half and oxidation normalization

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, ChestType};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::BlockPlaceContext;
use crate::behavior::block::{BlockBehavior, schedule_water_tick_if_waterlogged};
use crate::world::ScheduledTickAccess;

/// Copper chest behaviour
#[block_behavior]
pub struct CopperChestBlock {
    block: BlockRef,
}

impl CopperChestBlock {
    /// creates a new chest behaviour
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for CopperChestBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // TODO: Resolve facing waterlogging and doublechest placement (including secondary use)
        // TODO: Normalize connected halves to the least oxidized state and reconcile waxing
        Some(self.block.default_state())
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);
        update_shape(state, direction, neighbor_state)
    }
}

/// Copper chest behaviour
#[block_behavior]
pub struct WeatheringCopperChestBlock {
    block: BlockRef,
}

impl WeatheringCopperChestBlock {
    /// creates a new chest behaviour
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for WeatheringCopperChestBlock {
    // TODO: Implement weathering random ticks skipping right halves and chests with active viewers
    // TODO: Expose the weathering age to neighboring copper blocks through the shared capability
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // TODO: Resolve facing waterlogging and doublechest placement (including secondary use)
        // TODO: Normalize connected halves to the least oxidized state and reconcile waxing
        Some(self.block.default_state())
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);
        update_shape(state, direction, neighbor_state)
    }
}

fn update_shape(
    mut state: BlockStateId,
    direction: Direction,
    neighbor_state: BlockStateId,
) -> BlockStateId {
    let neighbor_type = neighbor_state
        .get_block()
        .has_tag(&BlockTag::COPPER_CHESTS)
        .then(|| neighbor_state.try_get_value(&BlockStateProperties::CHEST_TYPE))
        .flatten();
    if let Some(neighbor_type) = &neighbor_type
        && direction.is_horizontal()
    {
        if state.get_value(&BlockStateProperties::CHEST_TYPE) == ChestType::Single
            && *neighbor_type != ChestType::Single
            && state.get_value(&BlockStateProperties::FACING)
                == neighbor_state.get_value(&BlockStateProperties::FACING)
            && connected_direction(neighbor_state) == direction.opposite()
        {
            let opposite_type = match neighbor_type {
                ChestType::Left => ChestType::Right,
                ChestType::Right => ChestType::Left,
                ChestType::Single => ChestType::Single,
            };
            state = state.set_value(&BlockStateProperties::CHEST_TYPE, opposite_type);
        }
    } else if connected_direction(state) == direction {
        state = state.set_value(&BlockStateProperties::CHEST_TYPE, ChestType::Single);
    }

    if neighbor_type.is_some()
        && state.get_value(&BlockStateProperties::CHEST_TYPE) != ChestType::Single
        && connected_direction(state) == direction
    {
        return neighbor_state.with_properties_of(state);
    }
    state
}

fn connected_direction(state: BlockStateId) -> Direction {
    let facing = state.get_value(&BlockStateProperties::FACING);
    if state.get_value(&BlockStateProperties::CHEST_TYPE) == ChestType::Left {
        facing.rotate_y_clockwise()
    } else {
        facing.rotate_y_counter_clockwise()
    }
}
