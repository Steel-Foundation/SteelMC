//! Lava fluid implementation.
//!
//! Based on vanilla's LavaFluid.java.
//!
// TODO: Consider extracting common fluid behavior into a macro or generic implementation
//       to reduce duplication between WaterFluid and LavaFluid
// TODO: Add doc comments for all private helper methods
// TODO: Consider moving can_spread_down/spread_down to a shared trait when more fluids are added

use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::level_events;
use steel_registry::sound_events;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_fluids;
use steel_utils::BlockPos;
use steel_utils::types::UpdateFlags;

use crate::world::World;

use super::{
    FluidBehaviour, FluidRef, FluidState, can_hold_any_fluid, fluid_state_to_block,
    get_fluid_state, get_new_liquid, get_spread, is_hole, is_lava, is_water, lava_id,
};

/// Lava fluid behavior.
pub struct LavaFluid;

impl LavaFluid {
    /// Checks if lava can spread down to the below position.
    fn can_spread_down(world: &World, pos: &BlockPos) -> bool {
        let below = pos.offset(0, -1, 0);

        if !world.is_in_valid_bounds(&below) {
            return false;
        }

        let below_state = world.get_block_state(&below);
        let below_block = below_state.get_block();

        // Can flow into air or replaceable
        if below_block.config.is_air || below_block.config.replaceable {
            return true;
        }

        // Can flow into same fluid type (using tag check for mod support)
        let below_fluid = get_fluid_state(world, &below);
        if is_lava(below_fluid.fluid_id) && !below_fluid.is_source() {
            return true;
        }

        false
    }

    /// Spreads lava downward.
    fn spread_down(
        &self,
        world: &World,
        pos: BlockPos,
        _fluid_state: FluidState,
        _current_tick: u64,
    ) -> bool {
        let below = pos.offset(0, -1, 0);

        if !world.is_in_valid_bounds(&below) {
            return false;
        }

        let below_fluid = get_fluid_state(world, &below);

        // Lava-water interaction: lava flowing down into water
        // this means that you create stone (using tag check for mod support)
        if is_water(below_fluid.fluid_id) {
            let block_state = REGISTRY.blocks.get_default_state_id(vanilla_blocks::STONE);
            world.set_block(below, block_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);
            // Play fizz sound effect (level event 1501 - LAVA_FIZZ)
            // Using level_event for global sound that all nearby players can hear
            world.level_event(level_events::LAVA_FIZZ, below, 0, None);
            return true;
        }

        if !Self::can_spread_down(world, &pos) {
            return false;
        }

        // Calculate the correct fluid state for the below position
        let new_fluid = get_new_liquid(world, below, lava_id(), self.drop_off());

        if new_fluid.is_empty() {
            return false;
        }

        let block_state = fluid_state_to_block(new_fluid);

        if world.set_block(below, block_state, UpdateFlags::UPDATE_ALL_IMMEDIATE) {
            world.schedule_fluid_tick_default(
                below,
                &vanilla_fluids::LAVA,
                self.tick_delay() as i32,
            );
            return true;
        }

        false
    }

    /// Counts adjacent source blocks.
    fn source_neighbor_count(world: &World, pos: &BlockPos) -> u8 {
        let mut count = 0u8;

        for offset in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)] {
            let neighbor = pos.offset(offset.0, offset.1, offset.2);
            let fluid = get_fluid_state(world, &neighbor);
            if is_lava(fluid.fluid_id) && fluid.is_source() {
                count += 1;
            }
        }

        count
    }

    /// Animates the lava with ambient sounds.
    /// Based on vanilla's `LavaFluid.animateTick()`.
    ///
    /// Plays ambient sounds when air is above the lava.
    fn animate_tick(world: &World, pos: BlockPos, _fluid_state: FluidState) {
        // Check if air is above the lava
        let above_pos = pos.offset(0, 1, 0);
        let above_state = world.get_block_state(&above_pos);
        let above_block = above_state.get_block();

        if above_block.config.is_air {
            // 1/100 chance for lava pop sound
            if rand::random::<u8>().is_multiple_of(100) {
                let volume: f32 = rand::random::<f32>() * 0.2 + 0.9; // 0.9 to 1.1
                let pitch: f32 = rand::random::<f32>() * 0.2 + 0.9; // 0.9 to 1.1
                world.play_block_sound(sound_events::BLOCK_LAVA_POP, pos, volume, pitch, None);
            }

            // 1/200 chance for lava ambient sound
            if rand::random::<u8>().is_multiple_of(200) {
                let volume: f32 = rand::random::<f32>() * 0.2 + 0.9; // 0.9 to 1.1
                let pitch: f32 = rand::random::<f32>() * 0.2 + 0.9; // 0.9 to 1.1
                world.play_block_sound(sound_events::BLOCK_LAVA_AMBIENT, pos, volume, pitch, None);
            }
        }
    }

    /// Spreads lava to sides using vanilla's algorithm.
    fn spread_to_sides(
        &self,
        world: &World,
        pos: BlockPos,
        fluid_state: FluidState,
        _current_tick: u64,
        slope_find_distance: u8,
    ) {
        // Calculate spread amount - vanilla: fluidState.getAmount() - dropOff
        // Or 7 if falling (like level 1)
        let new_amount = if fluid_state.falling {
            7 // Falling water spreads at amount 7 (= level 1)
        } else {
            fluid_state.amount.saturating_sub(1)
        };

        if new_amount == 0 {
            return; // No more water to spread
        }

        // Get spread map using slope finding
        let spreads = get_spread(world, pos, lava_id(), self.drop_off(), slope_find_distance);

        for (direction, new_fluid) in spreads {
            let neighbor: BlockPos = direction.relative(&pos);

            // Check if the position can hold fluid
            if !can_hold_any_fluid(world, &neighbor) {
                continue;
            }

            // Check existing fluid
            let existing = get_fluid_state(world, &neighbor);

            // Check if existing fluid can be replaced
            if !existing.is_empty() {
                // Don't overwrite higher amount of same fluid type
                // For same fluid, we allow replacement if the new amount is higher
                // (this allows lava to "level up" as more sources contribute)
                // Using tag checks to support modded fluids
                if is_lava(existing.fluid_id) {
                    if existing.amount >= new_fluid.amount {
                        continue;
                    }
                    // Otherwise, allow replacement - lava can flow into lower-level lava
                } else if is_water(existing.fluid_id) {
                    // For water, check if lava can replace it
                    // Lava can replace water if lava height >= 0.44444445
                    if f32::from(fluid_state.amount) / 9.0 < 0.444_444_45 {
                        continue;
                    }
                    // Lava will replace water - play fizz sound
                    world.level_event(level_events::LAVA_FIZZ, neighbor, 0, None);
                } else {
                    // For other fluids/empty, check can_be_replaced_with
                    if !self.can_be_replaced_with(existing, world, neighbor, lava_id(), direction) {
                        continue;
                    }
                }
            }

            let block_state = fluid_state_to_block(new_fluid);

            if world.set_block(neighbor, block_state, UpdateFlags::UPDATE_ALL_IMMEDIATE) {
                world.schedule_fluid_tick_default(
                    neighbor,
                    &vanilla_fluids::LAVA,
                    self.tick_delay() as i32,
                );
            }
        }
    }
}

impl FluidBehaviour for LavaFluid {
    fn fluid_type(&self) -> FluidRef {
        lava_id()
    }

    fn tick_delay(&self) -> u32 {
        30
    }

    fn drop_off(&self) -> u8 {
        2
    }

    fn slope_find_distance(&self) -> u8 {
        2
    }

    fn tick(&self, world: &World, pos: BlockPos, current_tick: u64) {
        let tick_delay = 30;

        let mut current_fluid = get_fluid_state(world, &pos);

        if current_fluid.is_empty() || !is_lava(current_fluid.fluid_id) {
            return; // No lava here anymore
        }

        // Animate with ambient sounds (vanilla animateTick)
        Self::animate_tick(world, pos, current_fluid);

        // For flowing lava, recalculate if it should still exist
        if !current_fluid.is_source() {
            let new_fluid = get_new_liquid(world, pos, lava_id(), self.drop_off());

            if new_fluid.is_empty() {
                current_fluid = new_fluid;
                let air = fluid_state_to_block(FluidState::EMPTY);
                world.set_block(pos, air, UpdateFlags::UPDATE_ALL_IMMEDIATE);
            } else if new_fluid != current_fluid {
                // Update to new state
                current_fluid = new_fluid;
                let block_state = fluid_state_to_block(new_fluid);
                world.set_block(pos, block_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);

                world.schedule_fluid_tick_default(
                    pos,
                    &vanilla_fluids::LAVA,
                    tick_delay as i32,
                );
            }
        }

        // Spread using vanilla's algorithm
        self.spread(world, pos, current_fluid, current_tick);
    }

    fn spread(&self, world: &World, pos: BlockPos, fluid_state: FluidState, current_tick: u64) {
        if fluid_state.is_empty() {
            return;
        }

        let slope_find_distance = 2;

        // Vanilla spread() logic:
        // 1. Try to spread down
        // 2. If can spread down AND has 3+ source neighbors, also spread to sides
        // 3. Otherwise if source OR not a water hole below, spread to sides

        let can_spread_down = Self::can_spread_down(world, &pos);

        if can_spread_down {
            // Try to spread down
            let did_spread_down = self.spread_down(world, pos, fluid_state, current_tick);

            if did_spread_down {
                // If we have 3+ source neighbors, also spread to sides (source duplication)
                if Self::source_neighbor_count(world, &pos) >= 3 {
                    self.spread_to_sides(
                        world,
                        pos,
                        fluid_state,
                        current_tick,
                        slope_find_distance,
                    );
                }
                return;
            }
        }

        // If source OR not a lava hole below, spread to sides
        let is_lava_hole = is_hole(world, &pos, lava_id());

        if fluid_state.is_source() || !is_lava_hole {
            self.spread_to_sides(world, pos, fluid_state, current_tick, slope_find_distance);
        }
    }

    /// Returns true if lava can be replaced by another fluid.
    /// Based on vanilla `LavaFluid.canBeReplacedWith()`.
    /// Lava can be replaced if its height >= 0.44444445F and the fluid is water.
    fn can_be_replaced_with(
        &self,
        fluid_state: FluidState,
        _world: &World,
        _pos: BlockPos,
        other_fluid: FluidRef,
        _direction: Direction,
    ) -> bool {
        // Lava can be replaced if its height >= 0.44444445F (4/9 of a block)
        // and the replacing fluid is water (using tag check for mod support)
        let height = f32::from(fluid_state.amount) / 9.0; // Convert amount to height (0-1)
        height >= 0.444_444_45 && is_water(other_fluid)
    }
}
