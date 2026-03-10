//! Fluid state computation and source conversion logic.
//!
//! Equivalent to FlowingFluid#getNewLiquid and related helpers.

use crate::behavior::BlockStateBehaviorExt;
use crate::behavior::FLUID_BEHAVIORS;
use crate::fluid::can_pass_through_wall;
use crate::fluid::collision::{can_hold_fluid, can_hold_specific_fluid, can_pass_horizontally};
use crate::fluid::spread_context::SpreadContext;
use crate::world::World;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::fluid::{FluidRef, FluidState};
use steel_utils::BlockPos;

/// Calculates the new fluid state at a position based on neighbors.
#[must_use]
pub fn get_new_liquid(
    world: &World,
    pos: BlockPos,
    fluid_id: FluidRef,
    drop_off: u8,
) -> FluidState {
    let behavior = FLUID_BEHAVIORS.get_behavior(fluid_id);
    let mut max_incoming_amount = 0u8;
    let mut source_count = 0u8;

    for direction in [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
    ] {
        let neighbor_pos = direction.relative(&pos);
        let neighbor_fluid = world.get_block_state(&neighbor_pos).get_fluid_state();

        if !behavior.is_same(neighbor_fluid.fluid_id) {
            continue;
        }

        if !can_pass_through_wall(world, pos, neighbor_pos, direction) {
            continue;
        }

        if neighbor_fluid.is_source() {
            source_count += 1;
            max_incoming_amount = max_incoming_amount.max(8u8.saturating_sub(drop_off));
        } else {
            max_incoming_amount =
                max_incoming_amount.max(neighbor_fluid.amount.saturating_sub(drop_off));
        }
    }

    // Source conversion — delegate to the fluid's own canConvertToSource, which
    // encapsulates the game rule check (WATER/LAVA_SOURCE_CONVERSION).
    if source_count >= 2 && behavior.can_convert_to_source(world) {
        let below_pos = pos.below();
        let below_state = world.get_block_state(&below_pos);
        let below_fluid = below_state.get_fluid_state();
        if below_state.is_solid()
            || (behavior.is_same(below_fluid.fluid_id) && below_fluid.is_source())
        {
            return FluidState::source(fluid_id);
        }
    }

    // Check above for falling fluid
    let above_pos = pos.above();
    let above_fluid = world.get_block_state(&above_pos).get_fluid_state();
    if behavior.is_same(above_fluid.fluid_id)
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

/// Returns true if the position below is a hole (fluid can flow downward).
///
/// Vanilla equivalent: `FlowingFluid.isWaterHole()`.
/// Checks wall passability, then either same-fluid presence or `canHoldFluid`.
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
    let below_fluid = below_state.get_fluid_state();

    // Vanilla: bottomState.getFluidState().getType().isSame(this) ? true : canHoldFluid(...)
    if !below_fluid.is_empty()
        && FLUID_BEHAVIORS
            .get_behavior(fluid_id)
            .is_same(below_fluid.fluid_id)
    {
        return true;
    }

    can_hold_fluid(below_state, fluid_id)
}

/// Computes slope distance using DFS search.
///
/// Vanilla equivalent: `FlowingFluid.getSlopeDistance()`.
/// Uses `canPassThrough` = `canMaybePassThrough` + `canHoldSpecificFluid`.
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

        // Vanilla: canPassThrough = canMaybePassThrough + canHoldSpecificFluid
        if !ctx.can_pass_horizontally(neighbor, fluid_id) {
            continue;
        }

        if !can_pass_through_wall(ctx.world(), pos, neighbor, direction) {
            continue;
        }

        // canHoldSpecificFluid check (part of vanilla's canPassThrough)
        let neighbor_state = ctx.get_block_state(neighbor);
        if !can_hold_specific_fluid(neighbor_state, fluid_id) {
            continue;
        }

        // Vanilla parity: getSlopeDistance passes getFlowing() to canPassThrough,
        // and SimpleWaterloggedBlock.canPlaceLiquid only accepts source water
        // (fluid == Fluids.WATER). The DFS never traverses waterloggable blocks.
        if neighbor_state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
        {
            continue;
        }

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
///
/// Vanilla equivalent: `FlowingFluid.getSpread()`.
#[must_use]
pub fn get_spread(
    world: &World,
    pos: BlockPos,
    fluid_id: FluidRef,
    drop_off: u8,
    slope_find_distance: u8,
) -> Vec<(Direction, FluidState)> {
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
        let neighbor_state = world.get_block_state(&neighbor);

        // Vanilla: canMaybePassThrough (source check + canHoldAnyFluid + wall check)
        if !can_pass_horizontally(world, &neighbor, fluid_id) {
            continue;
        }
        if !can_pass_through_wall(world, pos, neighbor, direction) {
            continue;
        }

        // Calculate what fluid should exist at the neighbor position.
        let new_fluid = get_new_liquid(world, neighbor, fluid_id, drop_off);

        // Vanilla: canHoldSpecificFluid check (after canMaybePassThrough, before getNewLiquid
        // in vanilla, but ordering doesn't matter since none have side effects)
        if !can_hold_specific_fluid(neighbor_state, new_fluid.fluid_id) {
            continue;
        }

        // Vanilla parity: canHoldSpecificFluid passes newFluid.getType() to canPlaceLiquid.
        // Waterloggable blocks only accept source water (fluid == Fluids.WATER), so flowing
        // water is rejected. Only allow waterloggable targets when the computed fluid is source.
        if neighbor_state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
            && !new_fluid.is_source()
        {
            continue;
        }

        // Skip if no valid fluid would be placed.
        if new_fluid.is_empty() {
            continue;
        }

        // Calculate slope distance.
        let distance = if is_hole(world, &neighbor, fluid_id) {
            0
        } else if slope_find_distance > 0 {
            let ctx = ctx.get_or_insert_with(|| SpreadContext::new(world, pos));
            get_slope_distance(
                ctx,
                neighbor,
                1,
                Some(direction),
                fluid_id,
                slope_find_distance,
            )
        } else {
            1000
        };

        // Vanilla inline: if (distance < lowest) result.clear(); if (distance <= lowest) ...
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
            let existing = world.get_block_state(&neighbor).get_fluid_state();

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
