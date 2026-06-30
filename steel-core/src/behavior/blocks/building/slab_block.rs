//! Slab block behavior implementation.
//!
//! Slabs choose top/bottom/double shape during placement and only single slabs
//! implement vanilla waterlogging.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt as _,
        properties::{BlockStateProperties, Direction, SlabType},
    },
    fluid::{FluidRef, FluidState},
    vanilla_fluids,
};
use steel_utils::{BlockPos, BlockStateId};

use super::weathering_block::{WeatherState, WeatheringCopper};
use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, block::place_simple_waterlogged_liquid},
    world::{LevelAccessor, ScheduledTickAccess, World},
};

/// Behavior for slab blocks.
#[block_behavior]
pub struct SlabBlock {
    block: BlockRef,
}

impl SlabBlock {
    /// Creates a new slab block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn single_slab_type_for_placement(context: &BlockPlaceContext<'_>) -> SlabType {
        if context.clicked_face != Direction::Down
            && (context.clicked_face == Direction::Up
                || context.click_location.y - f64::from(context.relative_pos.y()) <= 0.5)
        {
            SlabType::Bottom
        } else {
            SlabType::Top
        }
    }
}

impl BlockBehavior for SlabBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let existing_state = context.world.get_block_state(context.relative_pos);
        if existing_state.get_block() == self.block {
            return Some(
                existing_state
                    .set_value(&BlockStateProperties::SLAB_TYPE, SlabType::Double)
                    .set_value(&BlockStateProperties::WATERLOGGED, false),
            );
        }

        Some(
            self.block
                .default_state()
                .set_value(
                    &BlockStateProperties::SLAB_TYPE,
                    Self::single_slab_type_for_placement(context),
                )
                .set_value(
                    &BlockStateProperties::WATERLOGGED,
                    context.is_water_source(),
                ),
        )
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

    fn can_place_liquid(&self, state: BlockStateId, fluid: FluidRef) -> bool {
        state.try_get_value(&BlockStateProperties::SLAB_TYPE) != Some(SlabType::Double)
            && fluid == &vanilla_fluids::WATER
    }

    fn place_liquid(
        &self,
        level: &dyn LevelAccessor,
        pos: BlockPos,
        state: BlockStateId,
        fluid_state: FluidState,
    ) -> bool {
        state.try_get_value(&BlockStateProperties::SLAB_TYPE) != Some(SlabType::Double)
            && place_simple_waterlogged_liquid(level, pos, state, fluid_state)
    }
}

/// Weathering copper slabs share slab shape/fluid behavior and add copper aging.
#[block_behavior]
pub struct WeatheringCopperSlabBlock {
    slab: SlabBlock,
    #[json_arg(r#enum = "WeatherState", json = "weather_state")]
    weathering: WeatheringCopper,
}

impl WeatheringCopperSlabBlock {
    /// Creates a new weathering copper slab behavior.
    #[must_use]
    pub const fn new(block: BlockRef, weather_state: WeatherState) -> Self {
        Self {
            slab: SlabBlock::new(block),
            weathering: WeatheringCopper::new(weather_state),
        }
    }
}

impl BlockBehavior for WeatheringCopperSlabBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.slab.get_state_for_placement(context)
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
        self.slab
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn can_place_liquid(&self, state: BlockStateId, fluid: FluidRef) -> bool {
        self.slab.can_place_liquid(state, fluid)
    }

    fn place_liquid(
        &self,
        level: &dyn LevelAccessor,
        pos: BlockPos,
        state: BlockStateId,
        fluid_state: FluidState,
    ) -> bool {
        self.slab.place_liquid(level, pos, state, fluid_state)
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
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::*;

    #[test]
    fn double_slabs_cannot_be_waterlogged() {
        init_test_registry();
        let behavior = SlabBlock::new(&vanilla_blocks::SMOOTH_STONE_SLAB);
        let double_slab = vanilla_blocks::SMOOTH_STONE_SLAB
            .default_state()
            .set_value(&BlockStateProperties::SLAB_TYPE, SlabType::Double)
            .set_value(&BlockStateProperties::WATERLOGGED, false);

        assert!(!behavior.can_place_liquid(double_slab, &vanilla_fluids::WATER));
    }

    #[test]
    fn single_slabs_accept_source_water_for_container_admission() {
        init_test_registry();
        let behavior = SlabBlock::new(&vanilla_blocks::SMOOTH_STONE_SLAB);
        let bottom_slab = vanilla_blocks::SMOOTH_STONE_SLAB
            .default_state()
            .set_value(&BlockStateProperties::SLAB_TYPE, SlabType::Bottom)
            .set_value(&BlockStateProperties::WATERLOGGED, false);

        assert!(behavior.can_place_liquid(bottom_slab, &vanilla_fluids::WATER));
    }

    #[test]
    fn single_slabs_reject_flowing_water_for_container_admission() {
        init_test_registry();
        let behavior = SlabBlock::new(&vanilla_blocks::SMOOTH_STONE_SLAB);
        let bottom_slab = vanilla_blocks::SMOOTH_STONE_SLAB
            .default_state()
            .set_value(&BlockStateProperties::SLAB_TYPE, SlabType::Bottom)
            .set_value(&BlockStateProperties::WATERLOGGED, false);

        assert!(!behavior.can_place_liquid(bottom_slab, &vanilla_fluids::FLOWING_WATER));
    }
}
