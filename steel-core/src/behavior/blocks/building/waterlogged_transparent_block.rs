use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::{
    BlockRef, block_state_ext::BlockStateExt as _, properties::BlockStateProperties,
};
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_fluids;
use steel_utils::{BlockPos, BlockStateId};

use super::weathering_block::{WeatherState, WeatheringCopper};
use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::world::{ScheduledTickAccess, World};

/// Vanilla `WaterloggedTransparentBlock` behavior.
#[block_behavior]
pub struct WaterloggedTransparentBlock {
    block: BlockRef,
}

impl WaterloggedTransparentBlock {
    /// Creates a new waterlogged transparent block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for WaterloggedTransparentBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state().set_value(
            &BlockStateProperties::WATERLOGGED,
            context.is_water_source(),
        ))
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
        if state.get_value(&BlockStateProperties::WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        state
    }

    fn get_fluid_state(&self, state: BlockStateId) -> FluidState {
        if state.get_value(&BlockStateProperties::WATERLOGGED) {
            FluidState::new(&vanilla_fluids::WATER, 8, true)
        } else {
            FluidState::EMPTY
        }
    }
}

/// Vanilla `WeatheringCopperGrateBlock` behavior.
#[block_behavior]
pub struct WeatheringCopperGrateBlock {
    transparent: WaterloggedTransparentBlock,
    #[json_arg(r#enum = "WeatherState", json = "weather_state")]
    weathering: WeatheringCopper,
}

impl WeatheringCopperGrateBlock {
    /// Creates a new weathering copper grate behavior.
    #[must_use]
    pub const fn new(block: BlockRef, weather_state: WeatherState) -> Self {
        Self {
            transparent: WaterloggedTransparentBlock::new(block),
            weathering: WeatheringCopper::new(weather_state),
        }
    }
}

impl BlockBehavior for WeatheringCopperGrateBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.transparent.get_state_for_placement(context)
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
        self.transparent
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn get_fluid_state(&self, state: BlockStateId) -> FluidState {
        self.transparent.get_fluid_state(state)
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        self.weathering.is_randomly_ticking()
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.weathering.change_over_time(state, world, pos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;

    #[test]
    fn waterlogged_transparent_block_returns_falling_source_water() {
        init_test_registry();
        let behavior = WaterloggedTransparentBlock::new(&vanilla_blocks::WAXED_COPPER_GRATE);
        let state = vanilla_blocks::WAXED_COPPER_GRATE
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);

        let fluid = behavior.get_fluid_state(state);

        assert_eq!(fluid.fluid_id, &vanilla_fluids::WATER);
        assert!(fluid.is_source());
        assert!(fluid.falling);
    }
}
