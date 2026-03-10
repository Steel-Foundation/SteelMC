//! Lava fluid callbacks.
//!
//! Based on vanilla's `LavaFluid.java`.
//! Implements `FluidBehavior` and `FlowingFluid` for sharing base spread logic.

use std::sync::Arc;

use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::game_rules::GameRuleValue;
use steel_registry::level_events;
use steel_registry::sound_events;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_dimension_types;
use steel_registry::vanilla_game_rules::LAVA_SOURCE_CONVERSION;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;
use steel_utils::types::UpdateFlags;

use crate::fluid::{FlowingFluid, FluidBehavior};
use crate::fluid::{
    FluidRef, FluidState, FluidStateExt, get_fluid_state, get_height, is_lava_fluid,
    is_water_fluid, lava_id,
};
use crate::world::World;
/// Lava fluid implementation.
///
/// Implements [`FluidBehavior`] with lava-specific parameters and
/// behaviors (dimension-dependent spread, uphill delay, lava/water chemistry,
/// fizz sounds).
pub struct LavaFluid;

impl LavaFluid {
    /// Returns true if this world uses fast lava (nether-like).
    // TODO: Vanilla uses EnvironmentAttributes.FAST_LAVA on the dimension type, not a hardcoded check
    fn is_fast_lava(world: &World) -> bool {
        world.dimension.key == vanilla_dimension_types::THE_NETHER.key
    }
}

impl FluidBehavior for LavaFluid {
    fn fluid_type(&self) -> FluidRef {
        lava_id()
    }

    fn is_same(&self, fluid: FluidRef) -> bool {
        is_lava_fluid(fluid)
    }

    fn tick_delay(&self, world: &World) -> i32 {
        if Self::is_fast_lava(world) { 10 } else { 30 }
    }

    fn drop_off(&self, world: &World) -> u8 {
        if Self::is_fast_lava(world) { 1 } else { 2 }
    }

    fn slope_find_distance(&self, world: &World) -> u8 {
        if Self::is_fast_lava(world) { 4 } else { 2 }
    }

    fn explosion_resistance(&self) -> f32 {
        100.0
    }

    /// Lava is randomly ticking for fire spread.
    fn is_randomly_ticking(&self) -> bool {
        true
    }

    fn can_convert_to_source(&self, world: &World) -> bool {
        match world.get_game_rule(LAVA_SOURCE_CONVERSION) {
            GameRuleValue::Bool(val) => val,
            GameRuleValue::Int(_) => false,
        }
    }

    /// Vanilla parity: `LavaFluid.canBeReplacedWith()`.
    /// Lava can be replaced if its effective height >= 0.444 and the replacer is water.
    ///
    /// Uses `get_height()` (not raw `amount / 9.0`) so that falling lava
    /// (same fluid directly above → height = 1.0) is also correctly handled.
    fn can_be_replaced_with(
        &self,
        fluid_state: FluidState,
        world: &World,
        pos: BlockPos,
        other_fluid: FluidRef,
        _direction: Direction,
    ) -> bool {
        get_height(world, &pos, fluid_state) >= 0.444_444_45 && is_water_fluid(other_fluid)
    }

    /// Vanilla parity: `LavaFluid.getSpreadDelay`.
    /// Uphill lava spreads 4× slower with 3/4 probability.
    ///
    /// "Uphill" means the target position (`new_state`) has a greater effective
    /// height than the source (`old_state`). Uses `get_height()` so that a
    /// falling lava source (height = 1.0) is correctly treated as "tall".
    fn get_spread_delay(
        &self,
        world: &World,
        pos: BlockPos,
        old_state: FluidState,
        new_state: FluidState,
    ) -> i32 {
        let base = self.tick_delay(world);
        if !old_state.is_empty()
            && !new_state.is_empty()
            && !old_state.falling
            && !new_state.falling
            && get_height(world, &pos, new_state) > get_height(world, &pos, old_state)
            && rand::random_range(0u32..4) != 0
        {
            base * 4
        } else {
            base
        }
    }

    /// Vanilla parity: `LavaFluid.beforeDestroyingBlock()` → fizz sound.
    /// Lava does NOT drop block items (unlike water).
    fn before_destroying_block(&self, world: &Arc<World>, pos: BlockPos, _state: BlockStateId) {
        world.level_event(level_events::LAVA_FIZZ, pos, 0, None);
    }

    /// Vanilla parity: `LavaFluid.animateTick()`.
    /// Plays pop (1/100) and ambient (1/200) sounds when air is above.
    fn animate_tick(&self, world: &World, pos: BlockPos, _state: FluidState) {
        let above_pos = pos.above();
        let above_block = world.get_block_state(&above_pos).get_block();

        if above_block.config.is_air {
            if rand::random_range(0u32..100) == 0 {
                let volume: f32 = rand::random::<f32>() * 0.2 + 0.2;
                let pitch: f32 = rand::random::<f32>() * 0.15 + 0.9;
                world.play_block_sound(sound_events::BLOCK_LAVA_POP, pos, volume, pitch, None);
            }

            if rand::random_range(0u32..200) == 0 {
                let volume: f32 = rand::random::<f32>() * 0.2 + 0.2;
                let pitch: f32 = rand::random::<f32>() * 0.15 + 0.9;
                world.play_block_sound(sound_events::BLOCK_LAVA_AMBIENT, pos, volume, pitch, None);
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
impl FlowingFluid for LavaFluid {
    fn spread_to(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        fluid_state: FluidState,
        direction: Direction,
    ) {
        if direction == Direction::Down {
            let below_fluid = get_fluid_state(world, &pos);
            if below_fluid.is_water() {
                // Vanilla: fizz always plays when lava meets water going down,
                // regardless of whether stone is formed.
                world.level_event(level_events::LAVA_FIZZ, pos, 0, None);

                // Vanilla: stone only forms when the target is a pure water LiquidBlock,
                // not a waterlogged block (stairs, slabs, etc.).
                let below_block = world.get_block_state(&pos).get_block();
                if below_block == vanilla_blocks::WATER {
                    world.set_block(
                        pos,
                        REGISTRY.blocks.get_default_state_id(vanilla_blocks::STONE),
                        UpdateFlags::UPDATE_ALL_IMMEDIATE,
                    );
                }
                return;
            }
        }

        self.base_spread_to(world, pos, fluid_state);
    }
}
