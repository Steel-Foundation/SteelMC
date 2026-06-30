use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{BlockRef, coral_plant_can_survive};

/// Vanilla `BaseCoralPlantBlock` survival (dead coral plants such as
/// `dead_tube_coral`).
///
/// Same `canSurvive` as `CoralPlantBlock`, without the death tick.
#[block_behavior]
pub struct BaseCoralPlantBlock {
    block: BlockRef,
}

impl BaseCoralPlantBlock {
    /// Creates a new dead coral plant block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for BaseCoralPlantBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        coral_plant_can_survive(world, pos)
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

        if direction == Direction::Down && !self.can_survive(state, world, pos) {
            vanilla_blocks::AIR.default_state()
        } else {
            state
        }
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        if !self.can_survive(state, context.world, context.relative_pos) {
            return None;
        }
        Some(state.set_value(&BlockStateProperties::WATERLOGGED, context.is_full_water()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestLevel;
    use steel_registry::test_support::init_test_registry;

    #[test]
    fn waterlogged_base_coral_plant_update_shape_schedules_water_tick() {
        init_test_registry();

        let behavior = BaseCoralPlantBlock::new(&vanilla_blocks::DEAD_TUBE_CORAL);
        let state = vanilla_blocks::DEAD_TUBE_CORAL
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);
        let level = TestLevel::default().with_block(
            BlockPos::ZERO.below(),
            vanilla_blocks::STONE.default_state(),
        );

        assert_eq!(
            behavior.update_shape(
                state,
                &level,
                BlockPos::ZERO,
                Direction::North,
                Direction::North.relative(BlockPos::ZERO),
                vanilla_blocks::AIR.default_state(),
            ),
            state
        );
        assert!(level.scheduled_water_tick());
    }
}
