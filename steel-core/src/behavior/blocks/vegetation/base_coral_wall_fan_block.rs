use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{BlockRef, coral_wall_fan_can_survive};

/// Vanilla `BaseCoralWallFanBlock` survival (dead coral wall fans).
#[block_behavior]
pub struct BaseCoralWallFanBlock {
    block: BlockRef,
}

impl BaseCoralWallFanBlock {
    /// Creates a new dead coral wall fan block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for BaseCoralWallFanBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        coral_wall_fan_can_survive(world, pos, facing)
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
        if state.get_value(&BlockStateProperties::WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        if direction.opposite() == state.get_value(&BlockStateProperties::HORIZONTAL_FACING)
            && !self.can_survive(state, world, pos)
        {
            vanilla_blocks::AIR.default_state()
        } else {
            state
        }
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self
            .block
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, context.is_full_water());

        context
            .get_nearest_looking_directions()
            .into_iter()
            .filter(|direction| direction.is_horizontal())
            .map(|direction| {
                state.set_value(
                    &BlockStateProperties::HORIZONTAL_FACING,
                    direction.opposite(),
                )
            })
            .find(|state| self.can_survive(*state, context.world, context.relative_pos))
    }
}
