//! bar block behavior implementation.
//!
//! bars connect to adjacent bars, bar solid blocks.

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, Direction};
use steel_registry::loot_table::DyeColor;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::building::{get_connection_state, update_shape};
use crate::behavior::context::BlockPlaceContext;
use crate::entity::ai::path::PathComputationType;
use crate::world::ScheduledTickAccess;
use steel_registry::vanilla_fluids;

/// All glass colored pane blocks
#[block_behavior]
pub struct StainedGlassPaneBlock {
    block: BlockRef,
    #[json_arg(
        r#enum = "DyeColor",
        json = "color",
        module = "steel_registry::loot_table"
    )]
    #[expect(unused, reason = "Is needed for beacon beam")]
    color: DyeColor,
}

/// Waterlogged property.
const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;

impl StainedGlassPaneBlock {
    /// Creates a new pane block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef, color: DyeColor) -> Self {
        Self { block, color }
    }
}

impl BlockBehavior for StainedGlassPaneBlock {
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
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }
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
