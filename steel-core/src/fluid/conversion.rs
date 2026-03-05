//! Fluid state computation and source conversion logic.
//!
//! Equivalent to FlowingFluid#getNewLiquid and related helpers.

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::game_rules::GameRuleValue;
use steel_utils::BlockPos;

use crate::behavior::fluid::FLUID_BEHAVIORS;
use crate::world::World;
use crate::fluid::{
    FluidBehavior,
    can_hold_any_fluid, can_pass_through_wall,
    water_id, lava_id, is_water, is_lava,
};
use crate::fluid::collision::can_pass_horizontally;
use crate::fluid::state::{get_fluid_state, get_fluid_state_from_block};
use crate::fluid::spread_context::SpreadContext;

/// Calculates the new fluid state at a position based on neighbors.
///
/// Equivalent to vanilla's `getNewLiquid()`.
///
/// # Vanilla ordering (parity)
/// 1. Horizontal neighbours — gated by `canPassThroughWall` so barriers like
///    glass panes / iron bars / walls are correctly opaque to fluid.
/// 2. Source conversion — checked **before** the above-fluid falling check.
///    In vanilla the `j >= 2 && canConvertToSource` branch returns early if
///    a new source forms, and only then falls through to the `UP` check.
/// 3. Above-fluid falling check — also gated by `canPassThroughWall(UP)`.
#[must_use]
pub fn get_new_liquid(
    world: &World,
    pos: BlockPos,
    fluid_id: FluidRef,
    drop_off: u8,
) -> FluidState {
    let mut max_incoming_amount = 0u8;
    let mut source_count = 0u8;

    // 1. Check each horizontal neighbour.
    //    Vanilla parity: canPassThroughWall must succeed before a neighbour
    //    is allowed to contribute its fluid level or source count.
    for direction in [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
    ] {
        let neighbor_pos = direction.relative(&pos);
        let neighbor_fluid = get_fluid_state(world, &neighbor_pos);

        if std::ptr::eq(neighbor_fluid.fluid_id, fluid_id) {
            // Parity: barriers between pos and the neighbour block fluid contribution.
            if !can_pass_through_wall(world, pos, neighbor_pos, direction) {
                continue;
            }

            if neighbor_fluid.is_source() {
                source_count += 1;
                // Source provides level 8 minus the drop-off.
                let incoming = 8u8.saturating_sub(drop_off);
                max_incoming_amount = max_incoming_amount.max(incoming);
            } else {
                // Flowing/falling fluid: propagate its level minus drop-off.
                let incoming = neighbor_fluid.amount.saturating_sub(drop_off);
                max_incoming_amount = max_incoming_amount.max(incoming);
            }
        }
    }

    // 2. Source conversion — must be evaluated BEFORE the above-fluid check
    //    (vanilla FlowingFluid.getNewLiquid lines 184-190 return before line 192).

    // Water source conversion: 2+ horizontal sources + solid/source below = new source.
    if is_water(fluid_id) && source_count >= 2 {
        use steel_registry::vanilla_game_rules::WATER_SOURCE_CONVERSION;
        let can_convert = match world.get_game_rule(WATER_SOURCE_CONVERSION) {
            GameRuleValue::Bool(val) => val,
            GameRuleValue::Int(_) => true,
        };
        if can_convert {
            let below_pos = pos.offset(0, -1, 0);
            let below_state = world.get_block_state(&below_pos);
            let below_block = below_state.get_block();
            let below_fluid = get_fluid_state_from_block(below_state);
            if (!below_block.config.replaceable && !below_block.config.is_air)
                || below_fluid.is_source()
            {
                return FluidState::source(water_id());
            }
        }
    }

    // Lava source conversion: same ordering requirement.
    if is_lava(fluid_id) && source_count >= 2 {
        use steel_registry::vanilla_game_rules::LAVA_SOURCE_CONVERSION;
        let can_convert = match world.get_game_rule(LAVA_SOURCE_CONVERSION) {
            GameRuleValue::Bool(val) => val,
            GameRuleValue::Int(_) => false,
        };
        if can_convert {
            let below_pos = pos.offset(0, -1, 0);
            let below_state = world.get_block_state(&below_pos);
            let below_block = below_state.get_block();
            let below_fluid = get_fluid_state_from_block(below_state);
            if (!below_block.config.replaceable && !below_block.config.is_air)
                || below_fluid.is_source()
            {
                return FluidState::source(lava_id());
            }
        }
    }

    // 3. Check above for falling fluid.
    //    Parity: vanilla also gates this on canPassThroughWall(UP) so a slab
    //    or trapdoor on top of a block doesn't erroneously create falling fluid.
    let above_pos = pos.offset(0, 1, 0);
    let above_fluid = get_fluid_state(world, &above_pos);
    if std::ptr::eq(above_fluid.fluid_id, fluid_id)
        && can_pass_through_wall(world, pos, above_pos, Direction::Up)
    {
        return FluidState::flowing(fluid_id, 8, true);
    }

    // If any horizontal flow reaches here, return the derived flowing state.
    if max_incoming_amount > 0 {
        FluidState::flowing(fluid_id, max_incoming_amount, false)
    } else {
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

    if !can_pass_through_wall(world, *pos, below, Direction::Down) {
        return false;
    }

    let below_state = world.get_block_state(&below);

    // Check if below is same fluid
    let below_fluid = get_fluid_state_from_block(below_state);
    if std::ptr::eq(below_fluid.fluid_id, fluid_id) && !below_fluid.is_source() {
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

/// Gets the spread map for a fluid, equivalent to vanilla's `FlowingFluid.getSpread()`.
///
/// Returns a list of `(Direction, FluidState)` pairs to spread to, filtered to
/// the directions with the shortest slope distance. For each candidate direction,
/// the target's existing `FluidState.canBeReplacedWith()` is checked before
/// adding it to the result — matching vanilla line 388:
/// ```java
/// if (fluidState.canBeReplacedWith(serverLevel, blockPos2, fluidState2.getType(), direction)) {
///     map.put(direction, fluidState2);
/// }
/// ```
#[must_use]
pub fn get_spread(
    world: &World,
    pos: BlockPos,
    fluid: &dyn FluidBehavior,
    drop_off: u8,
    slope_find_distance: u8,
) -> Vec<(Direction, FluidState)> {
    let fluid_id = fluid.fluid_type();
    let max_depth = slope_find_distance;
    let mut candidates: Vec<(Direction, FluidState, u16)> = Vec::new();

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
        // A waterloggable block (LiquidBlockContainer) can ONLY hold SOURCE water.
        // It cannot hold flowing water.
        let neighbor_state = world.get_block_state(&neighbor);
        if let Some(waterlogged) = neighbor_state.try_get_value(&steel_registry::blocks::properties::BlockStateProperties::WATERLOGGED) {
            // If it is already waterlogged, wait, canPlaceLiquid returns false if already waterlogged, 
            // but we only need to verify if the new fluid is a valid type to enter.
            // Vanilla: `!blockState.getValue(BlockStateProperties.WATERLOGGED) && fluid == Fluids.WATER`
            // If the fluid trying to spread into it is NOT a source block of water, it fails.
            if waterlogged || !new_fluid.is_source() || !crate::fluid::is_water(new_fluid.fluid_id) {
                continue;
            }
        }

        // Calculate slope distance.
        let distance = if is_hole(world, &neighbor, fluid_id) {
            0
        } else if max_depth > 0 {
            // Vanilla creates SpreadContext once per getSpread() call (reused across
            // directions for cross-direction caching). We create one per direction here,
            // which is slightly less efficient but keeps the origin correct.
            let mut ctx = SpreadContext::new(world, pos);
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

    // Find the minimum slope distance (vanilla: int i, updated via `if (j < i) map.clear()`).
    let min_distance = candidates.iter().map(|(_, _, d)| *d).min().unwrap_or(1000);

    // Return only directions with the minimum distance AND where the existing
    // fluid at the target allows replacement (vanilla line 388: canBeReplacedWith).
    //
    // IMPORTANT: vanilla calls `existingFluidState.canBeReplacedWith(...)`, i.e. the
    // check belongs to the EXISTING fluid, not the spreading fluid. We look up the
    // existing fluid's behavior via FLUID_BEHAVIORS.
    candidates
        .into_iter()
        .filter(|(dir, new_fluid, d)| {
            if *d != min_distance {
                return false;
            }
            let neighbor = dir.relative(&pos);
            let existing = get_fluid_state_from_block(world.get_block_state(&neighbor));
            // Vanilla: existingFluidState.canBeReplacedWith(world, pos, newFluid, direction)
            let existing_behavior = FLUID_BEHAVIORS.get_behavior(existing.fluid_id);
            existing_behavior.can_be_replaced_with(existing, world, neighbor, new_fluid.fluid_id, *dir)
        })
        .map(|(dir, fluid, _)| (dir, fluid))
        .collect()
}
