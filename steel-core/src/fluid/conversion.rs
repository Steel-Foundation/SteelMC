//! Fluid state computation and source conversion logic.
//!
//! Equivalent to FlowingFluid#getNewLiquid and related helpers.

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_game_rules::WATER_SOURCE_CONVERSION;
use steel_registry::vanilla_game_rules::LAVA_SOURCE_CONVERSION;
use steel_utils::BlockPos;
use steel_registry::blocks::properties::BlockStateProperties;
use crate::behavior::{BLOCK_BEHAVIORS, FLUID_BEHAVIORS};
use crate::fluid::collision::can_pass_horizontally;
use crate::fluid::spread_context::SpreadContext;
use crate::fluid::state::{get_fluid_state, get_fluid_state_from_block};
use crate::fluid::{
 can_hold_any_fluid, can_pass_through_wall, is_lava, is_water,
};
use crate::world::World;
use std::ptr;

/// Calculates the new fluid state at a position based on neighbors.
#[must_use]
pub fn get_new_liquid(
    world: &World,
    pos: BlockPos,
    fluid_id: FluidRef,
    drop_off: u8,
) -> FluidState {
    let mut max_incoming_amount = 0u8;
    let mut source_count = 0u8;

    for direction in [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
    ] {
        let neighbor_pos = direction.relative(&pos);
        let neighbor_fluid = get_fluid_state(world, &neighbor_pos);

        if !ptr::eq(neighbor_fluid.fluid_id, fluid_id) {
            continue;
        }

        if !can_pass_through_wall(world, pos, neighbor_pos, direction) {
            continue;
        }

        if neighbor_fluid.is_source() {
            source_count += 1;
            max_incoming_amount = max_incoming_amount.max(8u8.saturating_sub(drop_off));
        } else {
            max_incoming_amount = max_incoming_amount.max(neighbor_fluid.amount.saturating_sub(drop_off));
        }
    }

    // Source conversion
    let conversion_rule = if is_water(fluid_id) {
        Some((world.get_game_rule(WATER_SOURCE_CONVERSION), true))
    } else if is_lava(fluid_id) {
        Some((world.get_game_rule(LAVA_SOURCE_CONVERSION), false))
    } else {
        None
    };

    if let Some((rule, default)) = conversion_rule {
        if source_count >= 2 {
            let can_convert = match rule {
                GameRuleValue::Bool(val) => val,
                GameRuleValue::Int(_) => default,
            };
            if can_convert {
                let below_pos = pos.below();
                let below_state = world.get_block_state(&below_pos);
                let below_fluid = get_fluid_state_from_block(below_state);
                // Vanilla uses isSolid() (full collision shape) not a broader
                // non-replaceable/non-air check, so partial blocks (slabs, stairs)
                // do not trigger source conversion.
                // The source-below guard also requires the same fluid type to prevent
                // e.g. a lava source beneath flowing water from creating a water source.
                if below_state.is_solid()
                    || (ptr::eq(below_fluid.fluid_id, fluid_id) && below_fluid.is_source())
                {
                    return FluidState::source(fluid_id);
                }
            }
        }
    }

    // Check above for falling fluid
    let above_pos = pos.above();
    let above_fluid = get_fluid_state(world, &above_pos);
    if ptr::eq(above_fluid.fluid_id, fluid_id)
        && can_pass_through_wall(world, pos, above_pos, Direction::Up)
    {
        return FluidState::flowing(fluid_id, 8, true);
    }

    if max_incoming_amount > 0 {
        FluidState::flowing(fluid_id, max_incoming_amount, false)
    } else {
        FluidState::EMPTY
    }
}

/// Returns true if the position is a hole (fluid can flow downward).
#[must_use]
pub fn is_hole(world: &World, pos: &BlockPos, fluid_id: FluidRef) -> bool {
    let below = pos.below();

    if !world.is_in_valid_bounds(&below) {
        return false;
    }

    if !can_pass_through_wall(world, *pos, below, Direction::Down) {
        return false;
    }

    let below_state = world.get_block_state(&below);

    // Check if below is same fluid
    let below_fluid = get_fluid_state_from_block(below_state);
    if ptr::eq(below_fluid.fluid_id, fluid_id) && !below_fluid.is_source() {
        return true;
    }

    can_hold_any_fluid(world, &below)
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

        // Vanilla's canPassThrough also checks the wall between the current
        // exploration position and the neighbor (canPassThroughWall), not just
        // the target block's passability. Missing this causes fluids to
        // "see through" walls during slope finding.
        if !can_pass_through_wall(ctx.world(), pos, neighbor, direction) {
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

/// Gets the spread map for a fluid.
///
/// Returns a list of `(Direction, FluidState)` pairs to spread to, filtered to
/// the directions with the shortest slope distance. For each candidate direction,
/// the target's existing `FluidState.canBeReplacedWith()` is checked before
/// adding it to the result.
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
    // Lazily initialised on first use, matching vanilla's SpreadContext init.
    // Shared across all directions so cached block states and hole checks are
    // reused, matching vanilla's single-context-per-getSpread() behaviour.
    let mut ctx: Option<SpreadContext<'_>> = None;

    for direction in [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
    ] {
        let neighbor = direction.relative(&pos);

        // Can we flow there? (vanilla: canMaybePassThrough)
        if !can_pass_horizontally(world, &neighbor, fluid_id) {
            continue;
        }

        // Calculate what fluid should exist at the neighbor position.
        let new_fluid = get_new_liquid(world, neighbor, fluid_id, drop_off);

        // Skip if no valid fluid would be placed.
        if new_fluid.is_empty() {
            continue;
        }

        // Vanilla parity: canHoldSpecificFluid.
        // If the target is a LiquidBlockContainer (has WATERLOGGED), delegate to
        // can_place_liquid which encodes per-block acceptance rules.
        let neighbor_state = world.get_block_state(&neighbor);
        if neighbor_state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
        {
            let behavior = BLOCK_BEHAVIORS.get_behavior(neighbor_state.get_block());
            if !behavior.can_place_liquid(neighbor_state, new_fluid) {
                continue;
            }
        }

        // Calculate slope distance.
        let distance = if is_hole(world, &neighbor, fluid_id) {
            0
        } else if max_depth > 0 {
            let ctx = ctx.get_or_insert_with(|| SpreadContext::new(world, pos));
            get_slope_distance(ctx, neighbor, 1, Some(direction), fluid_id, max_depth)
        } else {
            1000
        };

        candidates.push((direction, new_fluid, distance));
    }

    if candidates.is_empty() {
        return Vec::new();
    }

    // Find the minimum slope distance
    let min_distance = candidates.iter().map(|(_, _, d)| *d).min().unwrap_or(1000);

    // Return only directions with the minimum distance AND where the existing
    // fluid at the target allows replacement.
    candidates
        .into_iter()
        .filter(|(dir, new_fluid, d)| {
            if *d != min_distance {
                return false;
            }
            let neighbor = dir.relative(&pos);
            let existing = get_fluid_state_from_block(world.get_block_state(&neighbor));

            let existing_behavior = FLUID_BEHAVIORS.get_behavior(existing.fluid_id);
            existing_behavior.can_be_replaced_with(
                existing,
                world,
                neighbor,
                new_fluid.fluid_id,
                *dir,
            )
        })
        .map(|(dir, fluid, _)| (dir, fluid))
        .collect()
}
