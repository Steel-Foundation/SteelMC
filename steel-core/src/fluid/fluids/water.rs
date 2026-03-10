//! Water fluid callbacks.
//!
//! Based on vanilla's `WaterFluid.java`.
//! Implements `FluidBehavior` and `FlowingFluid` for sharing base spread logic.

use std::sync::Arc;

use steel_registry::blocks::properties::Direction;
use steel_registry::game_rules::GameRuleValue;
use steel_registry::sound_events;
use steel_registry::vanilla_game_rules::WATER_SOURCE_CONVERSION;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use crate::fluid::{FlowingFluid, FluidBehavior};
use crate::fluid::{FluidRef, FluidState, is_water_fluid, water_id};
use crate::world::World;

/// Water fluid implementation.
///
/// Implements [`FluidBehavior`] with water-specific parameters and
/// behaviors (ambient sounds, lava→water chemistry, game-rule source conversion).
pub struct WaterFluid;

impl FluidBehavior for WaterFluid {
    fn fluid_type(&self) -> FluidRef {
        water_id()
    }

    fn is_same(&self, fluid: FluidRef) -> bool {
        is_water_fluid(fluid)
    }

    fn tick_delay(&self, _world: &World) -> i32 {
        5
    }

    fn drop_off(&self, _world: &World) -> u8 {
        1
    }

    fn slope_find_distance(&self, _world: &World) -> u8 {
        4
    }

    fn explosion_resistance(&self) -> f32 {
        100.0
    }

    fn can_convert_to_source(&self, world: &World) -> bool {
        match world.get_game_rule(WATER_SOURCE_CONVERSION) {
            GameRuleValue::Bool(val) => val,
            GameRuleValue::Int(_) => true,
        }
    }

    /// Water can only be replaced from below and only by non-water fluids.
    fn can_be_replaced_with(
        &self,
        _fluid_state: FluidState,
        _world: &World,
        _pos: BlockPos,
        other_fluid: FluidRef,
        direction: Direction,
    ) -> bool {
        direction == Direction::Down && !is_water_fluid(other_fluid)
    }

    /// Drops block resources and plays destruction particles when water replaces a non-air block.
    fn before_destroying_block(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        world.drop_resources(state, pos);
        world.destroy_block_effect(pos, u32::from(state.0), None);
    }

    /// Flowing water: 1/64 chance for ambient sound.
    /// Source water: 1/10 chance for underwater particles.
    fn animate_tick(&self, world: &World, pos: BlockPos, fluid_state: FluidState) {
        if !fluid_state.is_source() && !fluid_state.falling {
            // 1/64 chance for flowing water ambient sound
            if rand::random_range(0u32..64) == 0 {
                let volume: f32 = rand::random::<f32>() * 0.25 + 0.75;
                let pitch: f32 = rand::random::<f32>() + 0.5;
                world.play_block_sound(sound_events::BLOCK_WATER_AMBIENT, pos, volume, pitch, None);
            }
        } else {
            // 1/10 chance for underwater particles
            if rand::random_range(0u32..10) == 0 {
                // TODO: Spawn UNDERWATER particles (needs CLevelParticles packet)
            }
        }
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
