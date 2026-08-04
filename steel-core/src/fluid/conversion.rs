//! Fluid state computation and source conversion logic.
//!
//! Equivalent to FlowingFluid#getNewLiquid and related helpers.

use std::sync::Arc;

use crate::behavior::FLUID_BEHAVIORS;
use crate::fluid::can_pass_through_wall;
use crate::fluid::collision::{
    can_hold_fluid, can_hold_specific_fluid, can_pass_horizontally_internal,
};
use crate::fluid::spread_context::SpreadContext;
use crate::world::World;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::fluid::{FluidRef, FluidState};
use steel_utils::{BlockPos, BlockStateId};

/// Calculates the new fluid state at a position based on neighbors.
#[must_use]
pub fn get_new_liquid(
    world: &Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
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
        let neighbor_pos = direction.relative(pos);
        let neighbor_state = world.get_block_state(neighbor_pos);
        let neighbor_fluid = neighbor_state.get_fluid_state();

        if !behavior.is_same(neighbor_fluid.fluid_id) {
            continue;
        }

        if !can_pass_through_wall(world, pos, state, neighbor_pos, neighbor_state, direction) {
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
        let below_state = world.get_block_state(below_pos);
        let below_fluid = below_state.get_fluid_state();
        if below_state.is_solid()
            || (behavior.is_same(below_fluid.fluid_id) && below_fluid.is_source())
        {
            return FluidState::source(fluid_id.source_variant());
        }
    }

    // Check above for falling fluid
    let above_pos = pos.above();
    let above_state = world.get_block_state(above_pos);
    let above_fluid = above_state.get_fluid_state();
    if behavior.is_same(above_fluid.fluid_id)
        && can_pass_through_wall(world, pos, state, above_pos, above_state, Direction::Up)
    {
        return FluidState::flowing(fluid_id.flowing_variant(), 8, true);
    }

    if max_incoming_amount > 0 {
        FluidState::flowing(fluid_id.flowing_variant(), max_incoming_amount, false)
    } else {
        FluidState::EMPTY
    }
}

/// Returns true if the position below is a hole (fluid can flow downward).
///
/// Vanilla equivalent: `FlowingFluid.isWaterHole()`.
/// Checks wall passability, then either same-fluid presence or `canHoldFluid`.
#[must_use]
pub fn is_hole(
    world: &Arc<World>,
    top_pos: BlockPos,
    top_state: BlockStateId,
    bottom_pos: BlockPos,
    bottom_state: BlockStateId,
    fluid_id: FluidRef,
) -> bool {
    if !world.is_in_valid_bounds(bottom_pos) {
        return false;
    }

    if !can_pass_through_wall(
        world,
        top_pos,
        top_state,
        bottom_pos,
        bottom_state,
        Direction::Down,
    ) {
        return false;
    }

    can_flow_down_into(bottom_state, bottom_state.get_fluid_state(), fluid_id)
}

fn can_flow_down_into(
    below_state: steel_utils::BlockStateId,
    below_fluid: FluidState,
    fluid_id: FluidRef,
) -> bool {
    // Vanilla: bottomState.getFluidState().getType().isSame(this)
    //     ? true
    //     : canHoldFluid(..., this.getFlowing())
    if FLUID_BEHAVIORS
        .get_behavior(fluid_id)
        .is_same(below_fluid.fluid_id)
    {
        return true;
    }

    can_hold_fluid(below_state, fluid_id.flowing_variant())
}

/// Computes slope distance using DFS search.
///
/// Vanilla equivalent: `FlowingFluid.getSlopeDistance()`.
/// Uses `canPassThrough` = `canMaybePassThrough` + `canHoldSpecificFluid`.
#[must_use]
fn get_slope_distance(
    ctx: &mut SpreadContext,
    pos: BlockPos,
    state: BlockStateId,
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

        let neighbor = direction.relative(pos);
        let neighbor_state = ctx.get_block_state(neighbor);

        // Vanilla: canPassThrough = canMaybePassThrough + canHoldSpecificFluid
        if !can_pass_horizontally_internal(neighbor_state, fluid_id) {
            continue;
        }

        if !can_pass_through_wall(ctx.world(), pos, state, neighbor, neighbor_state, direction) {
            continue;
        }

        // canHoldSpecificFluid check (part of vanilla's canPassThrough).
        // getSlopeDistance passes getFlowing() to canPassThrough.
        if !can_hold_specific_fluid(neighbor_state, fluid_id.flowing_variant()) {
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
                neighbor_state,
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
    world: &Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
    fluid_id: FluidRef,
    drop_off: u8,
    slope_find_distance: u8,
) -> Vec<(Direction, FluidState)> {
    let mut candidates: Vec<(Direction, FluidState, FluidState, u16)> = Vec::new();
    // Lazily initialized on first use, matching vanilla's SpreadContext init.
    // Shared across all directions so cached block states and hole checks are
    // reused, matching vanilla's single-context-per-getSpread() behavior.
    let mut ctx: Option<SpreadContext<'_>> = None;

    for direction in [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
    ] {
        let neighbor = direction.relative(pos);
        let neighbor_state = match &mut ctx {
            Some(ctx) => ctx.get_block_state(neighbor),
            None => world.get_block_state(neighbor),
        };
        let neighbor_fluid = neighbor_state.get_fluid_state();

        // Vanilla: canMaybePassThrough (source check + canHoldAnyFluid + wall check)
        if !can_pass_horizontally_internal(neighbor_state, fluid_id) {
            continue;
        }
        if !can_pass_through_wall(world, pos, state, neighbor, neighbor_state, direction) {
            continue;
        }

        // Calculate what fluid should exist at the neighbor position.
        let new_fluid = get_new_liquid(world, neighbor, neighbor_state, fluid_id, drop_off);

        // Vanilla: canHoldSpecificFluid check after getNewLiquid.
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
        let ctx = ctx.get_or_insert_with(|| SpreadContext::new(world, pos));
        ctx.cache_block_state(neighbor, neighbor_state);
        let distance = if ctx.is_hole(neighbor, fluid_id) {
            0
        } else if slope_find_distance > 0 {
            get_slope_distance(
                ctx,
                neighbor,
                neighbor_state,
                1,
                Some(direction),
                fluid_id,
                slope_find_distance,
            )
        } else {
            1000
        };

        // Vanilla inline: if (distance < lowest) result.clear(); if (distance <= lowest) ...
        candidates.push((direction, new_fluid, neighbor_fluid, distance));
    }

    if candidates.is_empty() {
        return Vec::new();
    }

    // Find the minimum slope distance
    let min_distance = candidates
        .iter()
        .map(|(_, _, _, distance)| *distance)
        .min()
        .unwrap_or(1000);

    // Return only directions with the minimum distance AND where the existing
    // fluid at the target allows replacement.
    candidates
        .into_iter()
        .filter(|(direction, new_fluid, existing_fluid, distance)| {
            if *distance != min_distance {
                return false;
            }
            let neighbor = direction.relative(pos);

            let existing_behavior = FLUID_BEHAVIORS.get_behavior(existing_fluid.fluid_id);
            existing_behavior.can_be_replaced_with(
                *existing_fluid,
                world,
                neighbor,
                new_fluid.fluid_id,
                *direction,
            )
        })
        .map(|(direction, fluid, _, _)| (direction, fluid))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::behavior::init_behaviors;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
    use steel_registry::blocks::properties::BlockStateProperties;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks, vanilla_fluids};
    use steel_utils::ChunkPos;
    use steel_utils::types::UpdateFlags;

    use super::*;

    #[test]
    fn hole_check_rejects_dry_waterloggable_for_flowing_water_fallback() {
        init_test_registry();
        init_behaviors();

        let dry_waterloggable = vanilla_blocks::OAK_LEAVES
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, false);

        assert!(!can_flow_down_into(
            dry_waterloggable,
            FluidState::EMPTY,
            &vanilla_fluids::WATER
        ));
    }

    #[test]
    fn hole_check_treats_source_and_flowing_variants_as_same_fluid_below() {
        init_test_registry();
        init_behaviors();

        let flowing_water = vanilla_blocks::WATER
            .default_state()
            .set_value(&BlockStateProperties::LEVEL, 1);

        assert!(can_flow_down_into(
            flowing_water,
            flowing_water.get_fluid_state(),
            &vanilla_fluids::WATER
        ));
    }

    #[test]
    fn spread_keeps_the_closest_slope_even_when_its_target_rejects_replacement() {
        init_test_registry();
        init_behaviors();

        let world = fresh_test_world("fluid_spread_closest_slope");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let origin = BlockPos::new(8, 64, 8);
        let source_water = vanilla_blocks::WATER.default_state();
        let thin_lava = vanilla_blocks::LAVA
            .default_state()
            .set_value(&BlockStateProperties::LEVEL, 7);
        let stone = vanilla_blocks::STONE.default_state();
        let flags = UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE;

        for (pos, state) in [
            (origin, source_water),
            (origin.north(), thin_lava),
            (origin.south().below(), stone),
            (origin.east(), stone),
            (origin.west(), stone),
        ] {
            assert!(world.set_block_with_limit(pos, state, flags, 0));
        }

        let blocked = get_spread(&world, origin, source_water, &vanilla_fluids::WATER, 1, 4);
        assert!(blocked.is_empty());

        assert!(world.set_block_with_limit(
            origin.north(),
            vanilla_blocks::AIR.default_state(),
            flags,
            0,
        ));
        let open = get_spread(&world, origin, source_water, &vanilla_fluids::WATER, 1, 4);
        assert_eq!(
            open,
            vec![(
                Direction::North,
                FluidState::flowing(&vanilla_fluids::FLOWING_WATER, 7, false),
            )]
        );
    }
}
