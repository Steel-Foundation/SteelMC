use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext,
        blocks::{LightningRodBlock, WeatherState, WeatheringCopper},
    },
    entity::ai::path::PathComputationType,
    world::{LevelReader, ScheduledTickAccess, SignalQueryContext, World},
};

/// Behavior for all weathering lightning rod type blocks
#[block_behavior]
pub struct WeatheringLightningRodBlock {
    lightning_rod: LightningRodBlock,
    #[json_arg(r#enum = "WeatherState", json = "weather_state")]
    weathering: WeatheringCopper,
}

impl WeatheringLightningRodBlock {
    /// Creates a new weathering lightning rod block behavior for the given block
    #[must_use]
    pub const fn new(block: BlockRef, weather_state: WeatherState) -> Self {
        Self {
            lightning_rod: LightningRodBlock::new(block),
            weathering: WeatheringCopper::new(weather_state),
        }
    }
}

impl BlockBehavior for WeatheringLightningRodBlock {
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.weathering.change_over_time(state, world, pos);
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.lightning_rod.get_state_for_placement(context)
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
        self.lightning_rod
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn get_own_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        context: SignalQueryContext,
    ) -> i32 {
        self.lightning_rod
            .get_own_signal(state, world, pos, context)
    }

    fn get_direct_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
        context: SignalQueryContext,
    ) -> i32 {
        self.lightning_rod
            .get_direct_signal(state, world, pos, direction, context)
    }
    //TODO: override onLightningStrike() once it gets implemented

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.lightning_rod.tick(state, world, pos);
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        self.lightning_rod
            .affect_neighbors_after_removal(state, world, pos, moved_by_piston);
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        moved_by_piston: bool,
    ) {
        self.lightning_rod
            .on_place(state, world, pos, old_state, moved_by_piston);
    }

    fn is_signal_source(&self, state: BlockStateId, context: SignalQueryContext) -> bool {
        self.lightning_rod.is_signal_source(state, context)
    }

    fn is_pathfindable(&self, state: BlockStateId, computation_type: PathComputationType) -> bool {
        self.lightning_rod.is_pathfindable(state, computation_type)
    }
}
