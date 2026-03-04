//! Flowing fluid base implementation.
//!
//! Equivalent to Minecraft's FlowingFluid.java.
//! Contains full spreading + slope algorithm logic.

use steel_registry::blocks::properties::Direction;
use steel_utils::BlockPos;
use steel_utils::types::UpdateFlags;
use steel_registry::fluid::FluidState;

use crate::world::World;

use crate::fluid::FluidBehavior;
use crate::fluid::{
    get_fluid_state,
    can_hold_any_fluid,
    get_spread,
};

/// Base trait for flowing fluids (water, lava).
///
/// This implements the full vanilla flow algorithm.
/// Water/Lava only override parameters.
pub trait FlowingFluidBehavior: FluidBehavior {

    // ============================================================
    // ENTRY POINT
    // ============================================================

    /// Default tick implementation for flowing fluids.
    // TODO: PARITY: Fluid logic should not map directly to `tick` but to `scheduled_tick`.
    // Fluids must be enqueued with the Level's tick scheduler, with dimension-dependent delays.
    fn tick(&self, world: &World, pos: BlockPos, current_tick: u64) {
        let state = get_fluid_state(world, &pos);
        FlowingFluidBehavior::spread(self, world, pos, state, current_tick);
    }

    /// Core spread logic (vanilla equivalent of spread()).
    fn spread(
        &self,
        world: &World,
        pos: BlockPos,
        fluid_state: FluidState,
        _current_tick: u64,
    ) {
        if fluid_state.is_empty() {
            return;
        }

        // Try flowing downward first
        let flowed_down = self.try_flow_down(world, pos, fluid_state);
        
        // Determine if we should spread horizontally
        let mut should_flow_sideways = false;
        
        if flowed_down {
            // In vanilla, if we flow down, we also flow sideways ONLY IF we have 3+ source neighbors.
            let mut source_neighbors = 0;
            for dir in [Direction::North, Direction::South, Direction::West, Direction::East] {
                let neighbor_pos = dir.relative(&pos);
                if world.is_in_valid_bounds(&neighbor_pos) {
                    let neighbor_fluid = get_fluid_state(world, &neighbor_pos);
                    if std::ptr::eq(neighbor_fluid.fluid_id, self.fluid_type()) && neighbor_fluid.is_source() {
                        source_neighbors += 1;
                    }
                }
            }
            if source_neighbors >= 3 {
                should_flow_sideways = true;
            }
        } else {
            // In vanilla, if we don't flow down, we flow sideways if it's a source block 
            // OR if it's NOT a hole (meaning it can't flow down at all).
            if fluid_state.is_source() || !crate::fluid::is_hole(world, &pos, self.fluid_type()) {
                should_flow_sideways = true;
            }
        }
        
        if !should_flow_sideways {
            return;
        }

        // Spread horizontally using slope finding
        let spread = get_spread(
            world,
            pos,
            self.fluid_type(),
            self.drop_off(),
            self.slope_find_distance(),
        );

        for (direction, new_state) in spread {
            let target = direction.relative(&pos);
            let target_state = world.get_block_state(&target);
            // TODO: PARITY: Instead of immediate updates, flowing fluid might need to interact
            // with the block (extinguishing fire, dropping items).
            world.set_block(
                target,
                crate::fluid::state::fluid_state_to_block_with_existing(new_state, target_state),
                // TODO: PARITY: UpdateFlags in vanilla also notify neighbors and enqueue block updates
                UpdateFlags::UPDATE_ALL_IMMEDIATE,
            );
        }
    }

    // ============================================================
    // DOWNWARD FLOW
    // ============================================================

    fn try_flow_down(
        &self,
        world: &World,
        pos: BlockPos,
        _state: FluidState,
    ) -> bool {
        let below = pos.offset(0, -1, 0);

        if !world.is_in_valid_bounds(&below) {
            return false;
        }

        // TODO: PARITY: Lava & Water Chemistry. If lava flows into water (or vice versa),
        // we should generate stone/cobblestone/obsidian instead of just flowing.

        if !can_hold_any_fluid(world, &below) {
            return false;
        }

        let below_state = world.get_block_state(&below);
        let below_fluid = crate::fluid::get_fluid_state_from_block(below_state);

        if self.can_be_replaced_with(
            below_fluid,
            world,
            below,
            self.fluid_type(),
            Direction::Down,
        ) {
            let new_state = FluidState::flowing(
                self.fluid_type(),
                8,
                true,
            );
            world.set_block(
                below,
                crate::fluid::state::fluid_state_to_block_with_existing(new_state, below_state),
                UpdateFlags::UPDATE_ALL_IMMEDIATE,
            );

            return true;
        }

        false
    }
}