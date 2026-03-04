//! Water fluid implementation.
//!
//! Based on vanilla's WaterFluid.java.
//!
// TODO: Consider extracting common fluid behavior into a macro or generic implementation
//       to reduce duplication between WaterFluid and LavaFluid
// TODO: Add doc comments for all private helper methods
// TODO: Consider moving can_spread_down/spread_down to a shared trait when more fluids are added

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::level_events;
use steel_registry::sound_events;
use steel_registry::vanilla_fluids;
use steel_utils::BlockPos;
use steel_utils::types::UpdateFlags;

use crate::world::World;

use crate::fluid::{
    FluidBehavior, FluidRef, FluidState, can_hold_any_fluid, fluid_state_to_block,
    get_fluid_state, get_new_liquid, get_spread, is_hole, is_lava, is_water, water_id,
};

/// Water fluid behavior.
pub struct WaterFluid;

impl WaterFluid {
    /// Checks if water can spread down to the below position.
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
        if is_water(below_fluid.fluid_id) && !below_fluid.is_source() {
            return true;
        }

        false
    }

    /// Spreads water downward.
    fn spread_down(&self, world: &World, pos: BlockPos, _current_tick: u64) -> bool {
        let below = pos.offset(0, -1, 0);

        if !Self::can_spread_down(world, &pos) {
            return false;
        }

        // Calculate the correct fluid state for the below position
        let new_fluid = get_new_liquid(world, below, water_id(), self.drop_off());

        if new_fluid.is_empty() {
            return false;
        }

        let block_state = fluid_state_to_block(new_fluid);

        if world.set_block(below, block_state, UpdateFlags::UPDATE_ALL_IMMEDIATE) {
            world.schedule_fluid_tick_default(
                below,
                &vanilla_fluids::WATER,
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
            if is_water(fluid.fluid_id) && fluid.is_source() {
                count += 1;
            }
        }

        count
    }

    /// Animates the water with ambient sounds and particles.
    /// Based on vanilla's `WaterFluid.animateTick()`.
    ///
    /// For flowing water (not source, not falling): plays ambient sound with 1/64 chance
    /// For source water: spawns underwater particles with 1/10 chance
    fn animate_tick(world: &World, pos: BlockPos, fluid_state: FluidState) {
        // Check if this is flowing water (not source AND not falling)
        if !fluid_state.is_source() && !fluid_state.falling {
            // 1/64 chance to play ambient water sound (for flowing water)
            if rand::random::<u8>().is_multiple_of(64) {
                // Play water ambient sound
                // SoundSource::AMBIENT for environmental ambient sounds
                let volume: f32 = rand::random::<f32>() * 0.25 + 0.75; // 0.75 to 1.0
                let pitch: f32 = rand::random::<f32>() + 0.5; // 0.5 to 1.5
                world.play_block_sound(sound_events::BLOCK_WATER_AMBIENT, pos, volume, pitch, None);
            }
        } else {
            // For source water: 1/10 chance to spawn underwater particles
            if rand::random::<u8>().is_multiple_of(10) {
                // TODO: Spawn UNDERWATER particles
                // This requires:
                // 1. CLevelParticles packet implementation
                // 2. Particle type registry (ParticleTypes.UNDERWATER)
                // 3. World.spawn_particle() method
                //
                // Vanilla code:
                // level.addParticle(
                //     ParticleTypes.UNDERWATER,
                //     pos.x + random.nextDouble(),
                //     pos.y + random.nextDouble(),
                //     pos.z + random.nextDouble(),
                //     0.0, 0.0, 0.0
                // );
            }
        }
    }

    /// Spreads water to sides using vanilla's algorithm.
    fn spread_to_sides(
        &self,
        world: &World,
        pos: BlockPos,
        fluid_state: FluidState,
        _current_tick: u64,
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
        let spreads = get_spread(
            world,
            pos,
            water_id(),
            self.drop_off(),
            self.slope_find_distance(),
        );

        for (direction, new_fluid) in spreads {
            let neighbor: BlockPos = direction.relative(&pos);

            // Check if the position can hold fluid
            if !can_hold_any_fluid(world, &neighbor) {
                continue;
            }

            // Check existing fluid
            let existing = get_fluid_state(world, &neighbor);

            // Lava-water interaction: water flowing into lava creates obsidian/cobblestone
            // Using tag check to support modded fluids in the lava tag
            if is_lava(existing.fluid_id) {
                use steel_registry::REGISTRY;
                use steel_registry::vanilla_blocks;

                // If lava is source -> obsidian, otherwise -> cobblestone
                let is_lava_source = existing.is_source();
                let new_block = if is_lava_source {
                    vanilla_blocks::OBSIDIAN
                } else {
                    vanilla_blocks::COBBLESTONE
                };

                let block_state = REGISTRY.blocks.get_default_state_id(new_block);
                world.set_block(neighbor, block_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);
                // Play fizz sound effect (level event 1501 - LAVA_FIZZ)
                world.level_event(level_events::LAVA_FIZZ, neighbor, 0, None);
                continue;
            }

            // Check if existing fluid can be replaced
            if !existing.is_empty() {
                // Don't overwrite higher amount of same fluid type
                // For same fluid, we allow replacement if the new amount is higher
                // (this allows water to "level up" as more sources contribute)
                // Using tag checks to support modded fluids
                if is_water(existing.fluid_id) {
                    if existing.amount >= new_fluid.amount {
                        continue;
                    }
                    // Otherwise, allow replacement - water can flow into lower-level water
                } else if is_lava(existing.fluid_id) {
                    // For lava, we need to check if water can replace it
                    // This is handled by the lava-water interaction above, but let's be safe
                    if f32::from(existing.amount) / 9.0 >= 0.444_444_45 {
                        continue;
                    }
                    // Otherwise, obsidian/cobblestone will be created by the interaction check above
                } else {
                    // For other fluids/empty, check can_be_replaced_with
                    if !self.can_be_replaced_with(existing, world, neighbor, water_id(), direction)
                    {
                        continue;
                    }
                }
            }

            let block_state = fluid_state_to_block(new_fluid);

            if world.set_block(neighbor, block_state, UpdateFlags::UPDATE_ALL_IMMEDIATE) {
                world.schedule_fluid_tick_default(
                    neighbor,
                    &vanilla_fluids::WATER,
                    self.tick_delay() as i32,
                );
            }
        }
    }
}

impl FluidBehavior for WaterFluid {
    fn fluid_type(&self) -> FluidRef {
        water_id()
    }

    fn tick_delay(&self) -> u32 {
        5 // Water ticks every 5 ticks
    }

    fn drop_off(&self) -> u8 {
        1 // Water loses 1 level per block
    }

    fn slope_find_distance(&self) -> u8 {
        4 // Water searches up to 4 blocks for slope
    }

    fn tick(&self, world: &World, pos: BlockPos, current_tick: u64) {
        let mut current_fluid = get_fluid_state(world, &pos);
        log::info!("Water tick at {:?}", pos);

        if current_fluid.is_empty() || !is_water(current_fluid.fluid_id) {
            return; // No water here anymore
        }

        // Animate with ambient sounds and particles (vanilla animateTick)
        Self::animate_tick(world, pos, current_fluid);

        // For flowing water, recalculate if it should still exist
        log::info!("Current fluid informations at {:?}: fluid_id={}, amount={}, falling={}", pos, current_fluid.fluid_id.key, current_fluid.amount, current_fluid.falling);
        if !current_fluid.is_source() {
            log::info!("Recalculating flowing water at {:?} with amount {}", pos, current_fluid.amount);
            let new_fluid = get_new_liquid(world, pos, water_id(), self.drop_off());

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
                    &vanilla_fluids::WATER,
                    self.tick_delay() as i32,
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

        // Vanilla spread() logic:
        // 1. Try to spread down
        // 2. If can spread down AND has 3+ source neighbors, also spread to sides
        // 3. Otherwise if source OR not a water hole below, spread to sides

        let can_spread_down = Self::can_spread_down(world, &pos);

        if can_spread_down {
            // Try to spread down
            let did_spread_down = self.spread_down(world, pos, current_tick);

            if did_spread_down {
                // If we have 3+ source neighbors, also spread to sides (source duplication)
                if Self::source_neighbor_count(world, &pos) >= 3 {
                    self.spread_to_sides(world, pos, fluid_state, current_tick);
                }
                return;
            }
        }

        // If source OR not a water hole below, spread to sides
        let is_water_hole = is_hole(world, &pos, water_id());

        if fluid_state.is_source() || !is_water_hole {
            self.spread_to_sides(world, pos, fluid_state, current_tick);
        }
    }

    /// Returns true if water can be replaced by another fluid.
    /// Based on vanilla `WaterFluid.canBeReplacedWith()`.
    /// Water can only be replaced from DOWN direction and only by non-water fluids.
    fn can_be_replaced_with(
        &self,
        _fluid_state: FluidState,
        _world: &World,
        _pos: BlockPos,
        other_fluid: FluidRef,
        direction: Direction,
    ) -> bool {
        // Water can only be replaced from DOWN direction
        // and only by non-water fluids (using tag check for mod support)
        direction == Direction::Down && !is_water(other_fluid)
    }
}
