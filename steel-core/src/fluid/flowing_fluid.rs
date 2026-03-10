//! Shared logic for flowing fluids (Water, Lava).
//!
//! Provides the `FlowingFluid` trait, which contains the mathematical spread
//! algorithms derived from vanilla's `FlowingFluid.java`. Individual fluids
//! like `WaterFluid` and `LavaFluid` implement this trait to inherit behavior.

use std::sync::Arc;

use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::vanilla_blocks;
use steel_utils::BlockPos;
use steel_utils::types::UpdateFlags;

use crate::behavior::{BLOCK_BEHAVIORS, BlockStateBehaviorExt, FLUID_BEHAVIORS};
use crate::fluid::{
    FluidBehavior, FluidState, can_hold_any_fluid, can_hold_specific_fluid, can_pass_through_wall,
    fluid_state_to_block, fluid_state_to_block_with_existing, get_new_liquid, get_spread, is_hole,
};
use crate::world::World;

/// Trait providing the base algorithm for flowing fluids (Water, Lava).
/// In vanilla Minecraft, this is the `FlowingFluid` abstract class.
pub trait FlowingFluid: FluidBehavior {
    /// The base tick logic
    fn base_tick(&self, world: &Arc<World>, pos: BlockPos) {
        let mut current_fluid = world.get_block_state(&pos).get_fluid_state();

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
                // Vanilla: unconditionally sets Blocks.AIR when fluid empties
                let air = REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
                world.set_block(pos, air, UpdateFlags::UPDATE_ALL);
            } else if new_fluid != current_fluid {
                let old_fluid = current_fluid;
                current_fluid = new_fluid;
                let existing_state = world.get_block_state(&pos);
                let block_state = fluid_state_to_block_with_existing(new_fluid, existing_state);
                world.set_block(pos, block_state, UpdateFlags::UPDATE_ALL);

                world.schedule_fluid_tick_default(
                    pos,
                    self.fluid_type(),
                    self.get_spread_delay(world, pos, old_fluid, new_fluid),
                );
            }
        }

        self.spread(world, pos, current_fluid);
    }

    /// The base spread logic.
    ///
    /// Vanilla equivalent: `FlowingFluid.spread()`.
    fn base_spread(&self, world: &Arc<World>, pos: BlockPos, fluid_state: FluidState) {
        if fluid_state.is_empty() {
            return;
        }

        let below = pos.below();
        let below_state = world.get_block_state(&below);
        let below_fluid = below_state.get_fluid_state();

        // Vanilla: canMaybePassThrough (source check + canHoldAnyFluid + wall check)
        //          + canBeReplacedWith + canHoldSpecificFluid
        let can_spread_down = world.is_in_valid_bounds(&below)
            && !(self.is_same(below_fluid.fluid_id) && below_fluid.is_source())
            && can_hold_any_fluid(world, &below)
            && can_pass_through_wall(world, pos, below, Direction::Down);

        if can_spread_down {
            let new_below_fluid =
                get_new_liquid(world, below, self.fluid_type(), self.drop_off(world));

            if !new_below_fluid.is_empty() {
                let existing_behavior = FLUID_BEHAVIORS.get_behavior(below_fluid.fluid_id);
                let can_replace = existing_behavior.can_be_replaced_with(
                    below_fluid,
                    world,
                    below,
                    new_below_fluid.fluid_id,
                    Direction::Down,
                );

                if can_replace && can_hold_specific_fluid(below_state, new_below_fluid.fluid_id) {
                    self.spread_to(world, below, new_below_fluid, Direction::Down);

                    if self.source_neighbor_count(world, &pos) >= 3 {
                        self.spread_to_sides(world, pos, fluid_state);
                    }
                    return;
                }
            }
        }

        if fluid_state.is_source() || !is_hole(world, &pos, self.fluid_type()) {
            self.spread_to_sides(world, pos, fluid_state);
        }
    }

    /// The base logic for placing a fluid into a specific adjacent block.
    ///
    /// Vanilla equivalent: `FlowingFluid.spreadTo()`.
    /// Note: vanilla's spreadTo does NOT schedule ticks — that's handled by
    /// LiquidBlockContainer.placeLiquid or the new block's onPlace callback.
    fn base_spread_to(&self, world: &Arc<World>, pos: BlockPos, fluid_state: FluidState) {
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
        // Vanilla uses flag 3 (UPDATE_ALL). Tick scheduling is handled by
        // LiquidBlock.on_place which fires from set_block.
        world.set_block(pos, block_state, UpdateFlags::UPDATE_ALL);
    }

    /// Performs the actual placement of fluid and schedules the tick.
    fn spread_to(
        &self,
        world: &Arc<World>,
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
            let f = world.get_block_state(&neighbor).get_fluid_state();
            if self.is_same(f.fluid_id) && f.is_source() {
                count += 1;
            }
        }
        count
    }

    /// Spreads the fluid to horizontal neighbors.
    ///
    /// Vanilla equivalent: `FlowingFluid.spreadToSides()`.
    /// Computes outgoing amount, overrides to 7 for falling fluids, and skips
    /// if the outgoing amount is zero.
    fn spread_to_sides(&self, world: &Arc<World>, pos: BlockPos, fluid_state: FluidState) {
        // Vanilla: neighbor = amount - dropOff; if (falling) neighbor = 7; if (neighbor <= 0) skip
        let mut neighbor = fluid_state.amount.saturating_sub(self.drop_off(world));
        if fluid_state.falling {
            neighbor = 7;
        }
        if neighbor == 0 {
            return;
        }

        let spreads = get_spread(
            world,
            pos,
            self.fluid_type(),
            self.drop_off(world),
            self.slope_find_distance(world),
        );

        for (direction, new_fluid) in spreads {
            let target: BlockPos = direction.relative(&pos);
            self.spread_to(world, target, new_fluid, direction);
        }
    }
}
