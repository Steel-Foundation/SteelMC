use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::{BlockRef, coral_plant_can_survive, coral_scan_for_water, schedule_coral_die_tick};

/// Vanilla `CoralFanBlock` survival (live coral fans).
///
/// Inherits `canSurvive` from `BaseCoralPlantTypeBlock` via `BaseCoralFanBlock`.
#[block_behavior]
pub struct CoralFanBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks, json = "dead_block")]
    dead_block: BlockRef,
}

impl CoralFanBlock {
    /// Creates a new live coral fan block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, dead_block: BlockRef) -> Self {
        Self { block, dead_block }
    }

    fn dead_state(&self) -> BlockStateId {
        self.dead_block
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, false)
    }
}

impl BlockBehavior for CoralFanBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        coral_plant_can_survive(world, pos)
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        schedule_coral_die_tick(state, world, pos, self.block);
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
        if direction == Direction::Down && !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }

        schedule_coral_die_tick(state, world, pos, self.block);

        if state.get_value(&BlockStateProperties::WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        if !self.can_survive(state, context.world, context.relative_pos) {
            return None;
        }
        Some(state.set_value(&BlockStateProperties::WATERLOGGED, context.is_full_water()))
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !coral_scan_for_water(state, world, pos) {
            world.set_block(pos, self.dead_state(), UpdateFlags::UPDATE_CLIENTS);
        }
    }
}
