use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::{BlockRef, coral_scan_for_water, coral_wall_fan_can_survive, schedule_coral_die_tick};

/// Vanilla `CoralWallFanBlock` survival (live coral wall fans).
///
/// Inherits `canSurvive` from `BaseCoralWallFanBlock`.
#[block_behavior]
pub struct CoralWallFanBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks, json = "dead_block")]
    dead_block: BlockRef,
}

impl CoralWallFanBlock {
    /// Creates a new live coral wall fan block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, dead_block: BlockRef) -> Self {
        Self { block, dead_block }
    }

    fn dead_state(&self, state: BlockStateId) -> BlockStateId {
        self.dead_block
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, false)
            .set_value(
                &BlockStateProperties::HORIZONTAL_FACING,
                state.get_value(&BlockStateProperties::HORIZONTAL_FACING),
            )
    }
}

impl BlockBehavior for CoralWallFanBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        coral_wall_fan_can_survive(world, pos, facing)
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
        if direction.opposite() == state.get_value(&BlockStateProperties::HORIZONTAL_FACING)
            && !self.can_survive(state, world, pos)
        {
            return vanilla_blocks::AIR.default_state();
        }

        if state.get_value(&BlockStateProperties::WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        schedule_coral_die_tick(state, world, pos, self.block);
        state
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

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !coral_scan_for_water(state, world, pos) {
            world.set_block(pos, self.dead_state(state), UpdateFlags::UPDATE_CLIENTS);
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use super::*;

    #[test]
    fn dead_wall_fan_state_preserves_facing() {
        init_test_registry();
        let behavior = CoralWallFanBlock::new(
            &vanilla_blocks::TUBE_CORAL_WALL_FAN,
            &vanilla_blocks::DEAD_TUBE_CORAL_WALL_FAN,
        );
        let state = vanilla_blocks::TUBE_CORAL_WALL_FAN
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::West)
            .set_value(&BlockStateProperties::WATERLOGGED, true);

        let dead_state = behavior.dead_state(state);

        assert_eq!(
            dead_state.get_value(&BlockStateProperties::HORIZONTAL_FACING),
            Direction::West
        );
        assert!(!dead_state.get_value(&BlockStateProperties::WATERLOGGED));
    }
}
