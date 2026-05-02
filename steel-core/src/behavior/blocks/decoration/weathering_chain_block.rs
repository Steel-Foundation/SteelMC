//! Weathering copper chain block behavior implementation.
//!
//! Copper chains are oriented blocks with an axis property that determines their direction.
//! They can be waterlogged and will weather (oxidize) over time unless waxed.

use crate::behavior::BlockBehavior;
use crate::behavior::blocks::{WeatherState, WeatheringCopper};
use crate::behavior::context::BlockPlaceContext;
use crate::world::World;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, EnumProperty};
use steel_utils::math::Axis;
use steel_utils::{BlockPos, BlockStateId};

/// Behavior for weathering copper chain blocks.
///
/// Copper chains have an axis property that is set based on which face was clicked
/// during placement, can be waterlogged, and will oxidize over time.
#[block_behavior]
pub struct WeatheringCopperChainBlock {
    block: BlockRef,
    #[json_arg(r#enum = "WeatherState", json = "weather_state")]
    weathering: WeatheringCopper,
}

impl WeatheringCopperChainBlock {
    /// Axis property for the chain orientation.
    pub const AXIS: EnumProperty<Axis> = BlockStateProperties::AXIS;
    /// Waterlogged property.
    pub const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;

    /// Creates a new weathering copper chain block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef, weather_state: WeatherState) -> Self {
        Self {
            block,
            weathering: WeatheringCopper::new(weather_state),
        }
    }
}

impl BlockBehavior for WeatheringCopperChainBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(&Self::AXIS, context.clicked_face.get_axis())
                .set_value(&Self::WATERLOGGED, context.is_water_source()),
        )
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        self.weathering.is_randomly_ticking()
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.weathering.change_over_time(state, world, pos);
    }
}
