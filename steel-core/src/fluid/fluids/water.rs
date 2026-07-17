//! Water fluid callbacks.
//!
//! Based on vanilla's `WaterFluid.java`.
//! Implements `FluidBehavior` and `FlowingFluid` for sharing base spread logic.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::properties::Direction;
use steel_registry::vanilla_game_rules::WATER_SOURCE_CONVERSION;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use crate::entity::{Entity, InsideBlockEffectCollector, InsideBlockEffectType};
use crate::fluid::{FlowingFluid, FluidBehavior, get_flow as flowing_fluid_flow};
use crate::fluid::{FluidRef, FluidState, is_water_fluid, water_id};
use crate::world::World;

/// Water fluid implementation.
///
/// Implements [`FluidBehavior`] with water-specific parameters and
/// behaviors (lava→water chemistry and game-rule source conversion).
pub struct WaterFluid;

impl FluidBehavior for WaterFluid {
    fn fluid_type(&self) -> FluidRef {
        water_id()
    }

    fn is_same(&self, fluid: FluidRef) -> bool {
        is_water_fluid(fluid)
    }

    fn tick_delay(&self, _world: &Arc<World>) -> i32 {
        5
    }

    fn drop_off(&self, _world: &Arc<World>) -> u8 {
        1
    }

    fn slope_find_distance(&self, _world: &Arc<World>) -> u8 {
        4
    }

    fn explosion_resistance(&self) -> f32 {
        100.0
    }

    fn can_convert_to_source(&self, world: &Arc<World>) -> bool {
        world.get_game_rule(&WATER_SOURCE_CONVERSION)
    }

    fn get_flow(&self, world: &Arc<World>, pos: BlockPos, fluid_state: FluidState) -> DVec3 {
        flowing_fluid_flow(world, pos, fluid_state)
    }

    /// Water can only be replaced from below and only by non-water fluids.
    fn can_be_replaced_with(
        &self,
        _fluid_state: FluidState,
        _world: &Arc<World>,
        _pos: BlockPos,
        other_fluid: FluidRef,
        direction: Direction,
    ) -> bool {
        direction == Direction::Down && !is_water_fluid(other_fluid)
    }

    /// Drops block resources when water replaces a non-air block.
    fn before_destroying_block(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        world.drop_resources(state, pos);
    }

    /// Vanilla parity: `WaterFluid.entityInside()` extinguishes fire.
    fn entity_inside(
        &self,
        _world: &Arc<World>,
        _pos: BlockPos,
        _entity: &dyn Entity,
        effect_collector: &mut InsideBlockEffectCollector,
    ) {
        effect_collector.apply(InsideBlockEffectType::Extinguish);
    }

    fn tick(&self, world: &Arc<World>, pos: BlockPos) {
        self.base_tick(world, pos);
    }

    fn spread(&self, world: &Arc<World>, pos: BlockPos, fluid_state: FluidState) {
        self.base_spread(world, pos, fluid_state);
    }
}

// Marker impl to provide base FlowingFluid logic
impl FlowingFluid for WaterFluid {}
