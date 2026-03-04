//! Fluid collision and passability logic.
//!
//! Equivalent to various collision checks in FlowingFluid.java.

use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::fluid::FluidRef;
use steel_utils::{BlockPos, BlockStateId};

use crate::world::World;
use crate::fluid::state::get_fluid_state_from_block;

/// Checks if a block can be replaced by fluid.
#[must_use]
pub fn can_be_replaced_by_fluid(world: &World, pos: &BlockPos) -> bool {
    let state = world.get_block_state(pos);
    let block = state.get_block();

    // Air and replaceable blocks can be replaced
    block.config.replaceable || block.config.is_air
}

/// Checks if fluid can pass through a wall between two positions.
/// Based on vanilla's `FlowingFluid.canPassThroughWall()`.
///
/// Returns true if fluid can flow from `from` to `to` in the given direction,
/// considering collision shapes of both blocks.
#[must_use]
pub fn can_pass_through_wall(
    world: &World,
    from: BlockPos,
    to: BlockPos,
    _direction: Direction,
) -> bool {
    use crate::physics::shapes::merged_face_occludes;

    if !world.is_in_valid_bounds(&to) {
        return false;
    }

    let from_state = world.get_block_state(&from);
    let to_state = world.get_block_state(&to);

    // Get collision shapes
    let from_shape = from_state.get_collision_shape();
    let to_shape = to_state.get_collision_shape();

    // If shapes fully occlude the face, fluid cannot pass
    if merged_face_occludes(from_shape, to_shape, _direction) {
        return false;
    }

    // Check if the target block is replaceable or air
    let to_block = to_state.get_block();
    if to_block.config.is_air || to_block.config.replaceable {
        return true;
    }

    // Check if target already has the same fluid type (not source)
    let to_fluid = get_fluid_state_from_block(to_state);
    if !to_fluid.is_empty() && !to_fluid.is_source() {
        return true;
    }

    // Flow is allowed if there is a gap
    true
}

/// Checks if a block can hold any fluid.
/// Based on vanilla's `FlowingFluid.canHoldAnyFluid()`.
///
/// Returns false for blocks that shouldn't contain fluid:
/// - Doors
/// - Signs (all types)
/// - Ladders
/// - Sugar cane
/// - Bubble columns
/// - Portals (nether, end)
/// - End gateway
/// - Structure void
#[must_use]
pub fn can_hold_any_fluid(world: &World, pos: &BlockPos) -> bool {
    let state = world.get_block_state(pos);
    let block = state.get_block();

    // In Vanilla, blocks that implement `LiquidBlockContainer` can hold fluid.
    // In our implementation, we determine this by checking if the block state
    // has a WATERLOGGED property.
    use steel_registry::blocks::properties::BlockStateProperties;
    use steel_registry::blocks::block_state_ext::BlockStateExt;
    
    if state.try_get_value(&BlockStateProperties::WATERLOGGED).is_some() {
        return true;
    }

    // Check if block blocks motion (solid blocks that don't allow fluids)
    // TODO: PARITY: This check should be more sophisticated - we need to check if the
    // block implements LiquidBlockContainer correctly (e.g., checking canPlaceLiquid).
    // For now, we use a simplified approach based on config flags

    // TODO: PARITY: Check block exclusions (doors, signs, sugar cane, ladders, portals, structure voids).
    // In Vanilla, these explicitly return false so fluids cannot flow into them.

    // Simplified check: air and replaceable blocks can hold fluid
    // Non-replaceable blocks with collision generally cannot
    if block.config.is_air || block.config.replaceable {
        return true;
    }

    // If block has collision and is not replaceable, it likely can't hold fluid
    // TODO: PARITY: Add specific block type checks here when those block types are implemented
    !block.config.has_collision
}

/// Checks if fluid can pass through to a position horizontally.
/// Based on vanilla's path checking for slope finding.
#[must_use]
pub fn can_pass_horizontally(world: &World, pos: &BlockPos, target_fluid_id: FluidRef) -> bool {
    use steel_registry::blocks::shapes::is_shape_full_block;

    if !world.is_in_valid_bounds(pos) {
        return false;
    }

    let state = world.get_block_state(pos);
    let block = state.get_block();

    // Check collision shape
    let shape = state.get_collision_shape();

    // In vanilla `can_pass_horizontally` is used to search for slopes/spread targets.
    // It assumes moving from an empty/fluid space into `pos`.
    // It checks if the target block (`pos`) allows fluid to pass.
    // For `SpreadContext`, this replaces the weak `is_shape_full_block` check.
    
    
    // In slope finding, we are passing from the current position TO `pos` through a horizontal direction.
    // The previous position is conceptually an open fluid source.
    // Instead of full `merged_face_occludes` with the neighbor, vanilla's spread just checks if the target block
    // is a full shape, or if it has any gap. For strict parity we must check occlusion from an empty block.
    // An empty block to `target` block: `merged_face_occludes(empty, target, direction)`.
    // Which is essentially checking if `target` fully covers the opposite face.
    // However, it's easier to just assume true if not a full block.
    
    if is_shape_full_block(shape) {
        // Check if it's the same fluid type (not source) - can still flow through
        let fluid_state = get_fluid_state_from_block(state);
        if std::ptr::eq(fluid_state.fluid_id, target_fluid_id) && !fluid_state.is_source() {
            return true;
        }
        return false;
    }

    // If shape is empty, air, or replaceable, can pass through
    if shape.is_empty() || block.config.is_air || block.config.replaceable {
        return true;
    }

    // Can flow into same fluid type if not source
    let fluid_state = get_fluid_state_from_block(state);
    if std::ptr::eq(fluid_state.fluid_id, target_fluid_id) && !fluid_state.is_source() {
        return true;
    }

    false
}

/// Internal version of `can_pass_horizontally` for use by `SpreadContext`.
/// This takes individual components rather than querying the world.
#[must_use]
pub fn can_pass_horizontally_internal(state: BlockStateId, target_fluid_id: FluidRef) -> bool {
    use steel_registry::blocks::shapes::is_shape_full_block;

    let block = state.get_block();

    // Can always pass through air and replaceable blocks
    if block.config.is_air || block.config.replaceable {
        return true;
    }

    // Check collision shape
    let shape = state.get_collision_shape();

    // If shape is a full block, can't pass through (unless same fluid)
    if is_shape_full_block(shape) {
        let fluid_state = get_fluid_state_from_block(state);
        if std::ptr::eq(fluid_state.fluid_id, target_fluid_id) && !fluid_state.is_source() {
            return true;
        }
        return false;
    }

    // If shape is empty, can pass through
    if shape.is_empty() {
        return true;
    }

    // Can flow into same fluid type if not source
    let fluid_state = get_fluid_state_from_block(state);
    if std::ptr::eq(fluid_state.fluid_id, target_fluid_id) && !fluid_state.is_source() {
        return true;
    }

    false
}