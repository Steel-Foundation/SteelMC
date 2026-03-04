//! Fluid state computation and source conversion logic.
//!
//! Equivalent to FlowingFluid#getNewLiquid and related helpers.

use steel_registry::blocks::properties::Direction;
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::game_rules::GameRuleValue;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::BlockPos;

use crate::world::World;
use crate::fluid::{water_id, lava_id};
use crate::fluid::state::{get_fluid_state, get_fluid_state_from_block, is_water, is_lava};
use crate::fluid::spread_context::SpreadContext;

/// Calculates the new fluid state at a position based on neighbors.
///
/// Equivalent to vanilla's `getNewLiquid()`.
#[must_use]
pub fn get_new_liquid(
    world: &World,
    pos: BlockPos,
    fluid_id: FluidRef,
    drop_off: u8,
) -> FluidState {
    let mut max_incoming_amount = 0u8;
    let mut source_count = 0u8;

    // Check horizontal neighbors for water that could flow INTO this position
    for direction in [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
    ] {
        let neighbor_pos = direction.relative(&pos);
        let neighbor_fluid = get_fluid_state(world, &neighbor_pos);

        if std::ptr::eq(neighbor_fluid.fluid_id, fluid_id) {
            if neighbor_fluid.is_source() {
                source_count += 1;
                // Source can provide amount 8, minus drop_off
                let incoming = 8u8.saturating_sub(drop_off);
                max_incoming_amount = max_incoming_amount.max(incoming);
            } else {
                // Flowing water (including falling): calculate what amount it would provide
                // Falling water has amount=8, so it provides strong horizontal support
                let incoming = neighbor_fluid.amount.saturating_sub(drop_off);
                max_incoming_amount = max_incoming_amount.max(incoming);
            }
        }
    }

    // Check above for falling fluid - vanilla uses getFlowing(8, true)
    let above_pos = pos.offset(0, 1, 0);
    let above_fluid = get_fluid_state(world, &above_pos);
    if std::ptr::eq(above_fluid.fluid_id, fluid_id) {
        // Water above should create falling water here (level 8, falling=true)
        return FluidState::flowing(fluid_id, 8, true);
    }

    // Water source conversion: 2+ adjacent sources + solid below = new source
    // Check game rule for water source conversion (vanilla: default true)
    if is_water(fluid_id) && source_count >= 2 {
        use steel_registry::vanilla_game_rules::WATER_SOURCE_CONVERSION;
        let can_convert = match world.get_game_rule(WATER_SOURCE_CONVERSION) {
            GameRuleValue::Bool(val) => val,
            GameRuleValue::Int(_) => true, // Default to true if game rule not found
        };

        if can_convert {
            let below_pos = pos.offset(0, -1, 0);
            let below_state = world.get_block_state(&below_pos);
            let below_block = below_state.get_block();
            let below_fluid = get_fluid_state_from_block(below_state);
            // Solid block OR source of same type below
            if (!below_block.config.replaceable && !below_block.config.is_air)
                || below_fluid.is_source()
            {
                return FluidState::source(water_id());
            }
        }
    }

    // Lava source conversion: 2+ adjacent sources + solid below = new source
    // Check game rule for lava source conversion (vanilla: default false)
    if is_lava(fluid_id) && source_count >= 2 {
        use steel_registry::vanilla_game_rules::LAVA_SOURCE_CONVERSION;
        let can_convert = match world.get_game_rule(LAVA_SOURCE_CONVERSION) {
            GameRuleValue::Bool(val) => val,
            GameRuleValue::Int(_) => false, // Default to false if game rule not found
        };

        if can_convert {
            let below_pos = pos.offset(0, -1, 0);
            let below_state = world.get_block_state(&below_pos);
            let below_block = below_state.get_block();
            let below_fluid = get_fluid_state_from_block(below_state);
            // Solid block OR source of same type below
            if (!below_block.config.replaceable && !below_block.config.is_air)
                || below_fluid.is_source()
            {
                return FluidState::source(lava_id());
            }
        }
    }

    // If we have incoming flow, calculate new state
    if max_incoming_amount > 0 {
        let new_amount = max_incoming_amount;
        FluidState::flowing(fluid_id, new_amount, false)
    } else {
        // No support = empty
        FluidState::EMPTY
    }
}

/// Returns true if the position is a hole (fluid can flow downward).
#[must_use]
pub fn is_hole(
    world: &World,
    pos: &BlockPos,
    fluid_id: FluidRef,
) -> bool {
    let below = pos.offset(0, -1, 0);

    if !world.is_in_valid_bounds(&below) {
        return false;
    }

    if !crate::fluid::collision::can_pass_through_wall(world, *pos, below, Direction::Down) {
        return false;
    }

    let below_state = world.get_block_state(&below);

    // Check if below is same fluid
    let below_fluid = get_fluid_state_from_block(below_state);
    if std::ptr::eq(below_fluid.fluid_id, fluid_id) && !below_fluid.is_source() {
        return true;
    }

    crate::fluid::collision::can_hold_any_fluid(world, &below)
}

/// Computes slope distance using DFS/BFS search.
///
/// Equivalent to vanilla's slope finding algorithm.
#[must_use]
fn get_slope_distance(
    ctx: &mut SpreadContext,
    pos: BlockPos,
    depth: u8,
    from_direction: Option<Direction>,
    fluid_id: FluidRef,
    max_depth: u8,
) -> u16 {
    let mut min_distance: u16 = 1000;

    // Check all horizontal directions except the one we came from
    for direction in [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
    ] {
        // Skip the direction we came from
        if let Some(from) = from_direction
            && direction == from.opposite()
        {
            continue;
        }

        let neighbor = direction.relative(&pos);

        // Can we pass through to this neighbor?
        if !ctx.can_pass_horizontally(neighbor, fluid_id) {
            continue;
        }

        // Is this position a hole?
        if ctx.is_hole(neighbor, fluid_id) {
            return u16::from(depth); // Found a hole at this depth
        }

        // If we haven't reached max depth, continue searching
        if depth < max_depth {
            let distance = get_slope_distance(
                ctx,
                neighbor,
                depth + 1,
                Some(direction),
                fluid_id,
                max_depth,
            );
            if distance < min_distance {
                min_distance = distance;
            }
        }
    }

    min_distance
}

/// Gets the spread map for water, like vanilla's `getSpread()`.
///
/// Returns a list of (Direction, `FluidState`) pairs to spread to.
/// Uses slope finding to prioritize directions toward holes.
/// For each direction, calculates the correct `FluidState` using `get_new_liquid`.
#[must_use]
pub fn get_spread(
    world: &World,
    pos: BlockPos,
    fluid_id: FluidRef,
    drop_off: u8,
    slope_find_distance: u8,
) -> Vec<(Direction, FluidState)> {
    let max_depth = slope_find_distance;
    let mut candidates: Vec<(Direction, FluidState, u16)> = Vec::new();

    for direction in [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
    ] {
        let neighbor = direction.relative(&pos);

        // Can we flow there?
        if !crate::fluid::collision::can_pass_horizontally(world, &neighbor, fluid_id) {
            continue;
        }

        // Calculate what fluid should exist at the neighbor position
        // This is the key insight from vanilla - each position calculates its own state
        let new_fluid = get_new_liquid(world, neighbor, fluid_id, drop_off);

        // Skip if no valid fluid would be placed
        if new_fluid.is_empty() {
            continue;
        }

        // Calculate slope distance
        let distance = if is_hole(world, &neighbor, fluid_id) {
            0
        } else if max_depth > 0 {
            let mut ctx = SpreadContext::new(world);
            get_slope_distance(
                &mut ctx,
                neighbor,
                1,
                Some(direction),
                fluid_id,
                max_depth,
            )
        } else {
            1000
        };

        candidates.push((direction, new_fluid, distance));
    }

    if candidates.is_empty() {
        return Vec::new();
    }

    // Find the minimum distance
    let min_distance = candidates.iter().map(|(_, _, d)| *d).min().unwrap_or(1000);

    // Only return directions with the minimum distance (ties are allowed)
    candidates
        .into_iter()
        .filter(|(_, _, d)| *d == min_distance)
        .map(|(dir, fluid, _)| (dir, fluid))
        .collect()
}