//! Fluid collision and passability logic.
//!
//! Equivalent to various collision checks in FlowingFluid.java.

use crate::behavior::BLOCK_BEHAVIORS;
use crate::behavior::BlockStateBehaviorExt;
use crate::physics::shapes::merged_face_occludes;
use crate::world::World;
use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::blocks::properties::Direction;
use steel_registry::fluid::FluidRef;
use steel_registry::vanilla_block_tags::{DOORS_TAG, SIGNS_TAG};
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

// TODO: Add occlusion cache for performance (vanilla uses 200-entry ThreadLocal LRU)

/// Checks if fluid can pass through a wall between two positions.
#[must_use]
pub fn can_pass_through_wall(
    world: &World,
    from: BlockPos,
    to: BlockPos,
    direction: Direction,
) -> bool {
    if !world.is_in_valid_bounds(&to) {
        return false;
    }

    let from_shape = world.get_block_state(&from).get_collision_shape();
    let to_shape = world.get_block_state(&to).get_collision_shape();

    !merged_face_occludes(from_shape, to_shape, direction)
}

/// Checks if a block at the given world position can hold any fluid.
///
/// Vanilla equivalent: `FlowingFluid.canHoldAnyFluid(BlockState)`.
#[must_use]
pub fn can_hold_any_fluid(world: &World, pos: &BlockPos) -> bool {
    let state = world.get_block_state(pos);
    can_hold_any_fluid_state(state)
}

/// Checks if a block state can hold any fluid, without world access.
///
/// Vanilla equivalent: `FlowingFluid.canHoldAnyFluid(BlockState)`.
/// Uses `blocksMotion()` check instead of `has_collision` — vanilla's
/// `blocksMotion()` = `block != Cobweb && block != BambooSapling && isSolid()`.
#[must_use]
pub fn can_hold_any_fluid_state(state: BlockStateId) -> bool {
    let block = state.get_block();

    // Vanilla: block instanceof LiquidBlockContainer → true
    // Our equivalent: waterloggable blocks always accept fluid.
    if state
        .try_get_value(&BlockStateProperties::WATERLOGGED)
        .is_some()
    {
        return true;
    }

    // Vanilla: state.blocksMotion() ? false : !(exclusion list)
    if blocks_motion(block, state) {
        return false;
    }

    // Non-solid blocks that still reject fluid.
    !is_fluid_excluded_block(block)
}

/// Vanilla's `BlockState.blocksMotion()`:
/// `block != Cobweb && block != BambooSapling && isSolid()`
fn blocks_motion(block: BlockRef, state: BlockStateId) -> bool {
    block != vanilla_blocks::COBWEB && block != vanilla_blocks::BAMBOO_SAPLING && state.is_solid()
}

/// Returns true if a block is in the vanilla fluid exclusion list.
fn is_fluid_excluded_block(block: BlockRef) -> bool {
    block == vanilla_blocks::LADDER
        || block == vanilla_blocks::SUGAR_CANE
        || block == vanilla_blocks::BUBBLE_COLUMN
        || block == vanilla_blocks::NETHER_PORTAL
        || block == vanilla_blocks::END_PORTAL
        || block == vanilla_blocks::END_GATEWAY
        || block == vanilla_blocks::STRUCTURE_VOID
        || REGISTRY.blocks.is_in_tag(block, &SIGNS_TAG)
        || REGISTRY.blocks.is_in_tag(block, &DOORS_TAG)
}

/// Vanilla equivalent: `FlowingFluid.canHoldSpecificFluid(BlockGetter, BlockPos, BlockState, Fluid)`.
///
/// For `LiquidBlockContainer` blocks (blocks with WATERLOGGED), delegates to
/// `canPlaceLiquid(null, ...)`. For other blocks, always returns true.
#[must_use]
pub fn can_hold_specific_fluid(state: BlockStateId, fluid: FluidRef) -> bool {
    if state
        .try_get_value(&BlockStateProperties::WATERLOGGED)
        .is_some()
    {
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        behavior.can_place_liquid(state, fluid)
    } else {
        true
    }
}

/// Vanilla equivalent: `FlowingFluid.canHoldFluid(BlockGetter, BlockPos, BlockState, Fluid)`.
///
/// Combined check: `canHoldAnyFluid(state) && canHoldSpecificFluid(state, fluid)`.
#[must_use]
pub fn can_hold_fluid(state: BlockStateId, fluid: FluidRef) -> bool {
    can_hold_any_fluid_state(state) && can_hold_specific_fluid(state, fluid)
}

/// Checks if fluid can pass through to a position horizontally.
///
/// This is the world-querying entry point. It reads the block state at `pos`
/// and delegates entirely to [`can_pass_horizontally_internal`]
#[must_use]
pub fn can_pass_horizontally(world: &World, pos: &BlockPos, target_fluid_id: FluidRef) -> bool {
    if !world.is_in_valid_bounds(pos) {
        return false;
    }
    let state = world.get_block_state(pos);
    can_pass_horizontally_internal(state, target_fluid_id)
}

/// Core passability logic for horizontal fluid spread.
///
/// Vanilla equivalent: `!isSourceBlockOfThisType(testFluidState) && canHoldAnyFluid(testState)`.
///
/// Single source of truth used by both the world-querying
/// [`can_pass_horizontally`] and [`SpreadContext`] (which supplies a
/// cached `BlockStateId` to avoid redundant world lookups).
#[must_use]
pub fn can_pass_horizontally_internal(state: BlockStateId, target_fluid_id: FluidRef) -> bool {
    // Vanilla: !isSourceBlockOfThisType — reject same-type source blocks
    let fluid_state = state.get_fluid_state();
    if fluid_state.fluid_id == target_fluid_id && fluid_state.is_source() {
        return false;
    }

    // Vanilla: canHoldAnyFluid
    can_hold_any_fluid_state(state)
}
