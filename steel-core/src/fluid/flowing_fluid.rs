//! Shared logic for flowing fluids (Water, Lava).
//!
//! Provides the `FlowingFluid` trait, which contains the mathematical spread
//! algorithms derived from vanilla's `FlowingFluid.java`. Individual fluids
//! like `WaterFluid` and `LavaFluid` implement this trait to inherit behavior.

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_utils::{BlockPos, types::UpdateFlags};

use crate::behavior::{BLOCK_BEHAVIORS, FLUID_BEHAVIORS};
use crate::fluid::{
    FluidBehavior, FluidState, can_hold_any_fluid, can_pass_through_wall, fluid_state_to_block,
    fluid_state_to_block_with_existing, get_fluid_state, get_fluid_state_from_block,
    get_new_liquid, get_spread, is_hole,
};
use crate::world::World;

/// Trait providing the base algorithm for flowing fluids (Water, Lava).
/// In vanilla Minecraft, this is the `FlowingFluid` abstract class.
pub trait FlowingFluid: FluidBehavior {
    /// The base tick logic
    fn base_tick(&self, world: &World, pos: BlockPos) {
        let mut current_fluid = get_fluid_state(world, &pos);

        if current_fluid.is_empty() || !self.is_same(current_fluid.fluid_id) {
            return;
        }

        // TODO: animate_tick (ambient sounds, particles) belongs in a client-side
        // ambient tick dispatcher (equivalent to Level.animateTick), not here.
        // It should fire at render rate for nearby blocks, not per scheduled fluid tick.

        if !current_fluid.is_source() {
            let new_fluid = get_new_liquid(world, pos, self.fluid_type(), self.drop_off(world));

            if new_fluid.is_empty() {
                current_fluid = new_fluid;
                let existing_state = world.get_block_state(&pos);
                let air_or_unwaterlogged =
                    fluid_state_to_block_with_existing(FluidState::EMPTY, existing_state);
                world.set_block(pos, air_or_unwaterlogged, UpdateFlags::UPDATE_ALL_IMMEDIATE);
            } else if new_fluid != current_fluid {
                let old_fluid = current_fluid;
                current_fluid = new_fluid;
                let existing_state = world.get_block_state(&pos);
                let block_state = fluid_state_to_block_with_existing(new_fluid, existing_state);
                world.set_block(pos, block_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);

                world.schedule_fluid_tick_default(
                    pos,
                    self.fluid_type(),
                    self.get_spread_delay(world, pos, old_fluid, new_fluid),
                );
            }
        }

        self.spread(world, pos, current_fluid);
    }

    /// The base spread logic
    fn base_spread(&self, world: &World, pos: BlockPos, fluid_state: FluidState) {
        if fluid_state.is_empty() {
            return;
        }

        if self.can_spread_down(world, &pos) {
            let did_spread_down = self.spread_down(world, pos);

            if did_spread_down {
                if self.source_neighbor_count(world, &pos) >= 3 {
                    self.spread_to_sides(world, pos);
                }
                return;
            }
        }

        let is_fluid_hole = is_hole(world, &pos, self.fluid_type());

        if fluid_state.is_source() || !is_fluid_hole {
            self.spread_to_sides(world, pos);
        }
    }

    /// The base logic for placing a fluid into a specific adjacent block.
    fn base_spread_to(&self, world: &World, pos: BlockPos, fluid_state: FluidState) {
        let target_state = world.get_block_state(&pos);

        if target_state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
        {
            let behavior = BLOCK_BEHAVIORS.get_behavior(target_state.get_block());
            if behavior.place_liquid(world, pos, target_state, fluid_state) {
                return;
            }
        }

        // Non-LiquidBlockContainer path: destroy the block and place the raw fluid.
        let target_block = target_state.get_block();
        if !target_block.config.is_air {
            self.before_destroying_block(world, pos, target_state);
        }

        let block_state = fluid_state_to_block(fluid_state);
        if world.set_block(pos, block_state, UpdateFlags::UPDATE_ALL_IMMEDIATE) {
            world.schedule_fluid_tick_default(pos, self.fluid_type(), self.tick_delay(world));
        }
    }

    /// Performs the actual placement of fluid and schedules the tick.
    fn spread_to(
        &self,
        world: &World,
        pos: BlockPos,
        fluid_state: FluidState,
        _direction: Direction,
    ) {
        self.base_spread_to(world, pos, fluid_state);
    }

    /// Returns the number of fluid sources in the 4-directional neighborhood of the given position.
    fn source_neighbor_count(&self, world: &World, pos: &BlockPos) -> u8 {
        let mut count = 0u8;
        for dir in Direction::HORIZONTAL {
            let neighbor = dir.relative(pos);
            let f = get_fluid_state(world, &neighbor);
            if self.is_same(f.fluid_id) && f.is_source() {
                count += 1;
            }
        }
        count
    }

    /// Returns true if the fluid can spread down from the given position.
    fn can_spread_down(&self, world: &World, pos: &BlockPos) -> bool {
        let below = pos.below();

        if !world.is_in_valid_bounds(&below) {
            return false;
        }

        let below_fluid = get_fluid_state(world, &below);

        if self.is_same(below_fluid.fluid_id) && below_fluid.is_source() {
            return false;
        }

        if !can_hold_any_fluid(world, &below) {
            return false;
        }

        can_pass_through_wall(world, *pos, below, Direction::Down)
    }

    /// Spreads the fluid down from the given position.
    ///
    /// Callers must have already verified `can_spread_down` before calling this.
    fn spread_down(&self, world: &World, pos: BlockPos) -> bool {
        let below = pos.below();

        let new_fluid = get_new_liquid(world, below, self.fluid_type(), self.drop_off(world));
        if new_fluid.is_empty() {
            return false;
        }

        let below_state = world.get_block_state(&below);

        let existing_below = get_fluid_state_from_block(below_state);
        let existing_behavior = FLUID_BEHAVIORS.get_behavior(existing_below.fluid_id);
        if !existing_behavior.can_be_replaced_with(
            existing_below,
            world,
            below,
            new_fluid.fluid_id,
            Direction::Down,
        ) {
            return false;
        }

        if below_state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
        {
            let behavior = BLOCK_BEHAVIORS.get_behavior(below_state.get_block());
            if !behavior.can_place_liquid(below_state, new_fluid) {
                return false;
            }
        }

        self.spread_to(world, below, new_fluid, Direction::Down);
        true
    }

    /// Spreads the fluid to the given position.
    fn spread_to_sides(&self, world: &World, pos: BlockPos) {
        let spreads = get_spread(
            world,
            pos,
            self.fluid_type(),
            self.drop_off(world),
            self.slope_find_distance(world),
        );

        for (direction, new_fluid) in spreads {
            let neighbor: BlockPos = direction.relative(&pos);
            self.spread_to(world, neighbor, new_fluid, direction);
        }
    }
}
