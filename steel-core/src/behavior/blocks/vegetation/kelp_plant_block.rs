use steel_macros::block_behavior;
use steel_registry::blocks::properties::Direction;
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::vanilla_fluids;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{BlockRef, kelp_can_survive, water_source_fluid_state};

/// Vanilla `KelpPlantBlock` survival and fluid state.
// TODO: Implement full vanilla behavior beyond can_survive and get_fluid_state.
#[block_behavior]
pub struct KelpPlantBlock {
    block: BlockRef,
}

impl KelpPlantBlock {
    /// Creates a new kelp plant block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for KelpPlantBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        kelp_can_survive(world, pos)
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
        let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
        let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        (context.is_full_water() && self.can_survive(state, context.world, context.relative_pos))
            .then_some(state)
    }

    fn get_fluid_state(&self, _state: BlockStateId) -> FluidState {
        water_source_fluid_state()
    }

    fn is_liquid_container(&self, _state: BlockStateId) -> bool {
        true
    }

    fn can_place_liquid(&self, _state: BlockStateId, _fluid: FluidRef) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestLevel;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;

    #[test]
    fn kelp_plant_update_shape_schedules_water_tick() {
        init_test_registry();

        let kelp = KelpPlantBlock::new(&vanilla_blocks::KELP_PLANT);
        let level =
            TestLevel::default().with_default_block_state(vanilla_blocks::WATER.default_state());
        let state = vanilla_blocks::KELP_PLANT.default_state();

        assert_eq!(
            kelp.update_shape(
                state,
                &level,
                BlockPos::ZERO,
                Direction::North,
                Direction::North.relative(BlockPos::ZERO),
                vanilla_blocks::WATER.default_state(),
            ),
            state
        );
        assert!(level.scheduled_water_tick());
    }
}
