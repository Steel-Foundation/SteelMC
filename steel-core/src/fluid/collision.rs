//! Fluid collision and passability logic.
//!
//! Equivalent to various collision checks in FlowingFluid.java.

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::fluid::FluidRef;
use steel_registry::vanilla_blocks;
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
///
/// Direct equivalent of vanilla's `FlowingFluid.canPassThroughWall()`:
/// ```java
/// return !Shapes.mergedFaceOccludes(fromShape, toShape, direction);
/// ```
///
/// Returns `true` when the combined collision shapes of `from` and `to` do NOT
/// fully occlude their shared face — i.e. there is a geometrical gap for fluid
/// to flow through. Returns `false` when either block is a full cube or their
/// combined shapes seal the face.
#[must_use]
pub fn can_pass_through_wall(
    world: &World,
    from: BlockPos,
    to: BlockPos,
    direction: Direction,
) -> bool {
    use crate::physics::shapes::merged_face_occludes;

    if !world.is_in_valid_bounds(&to) {
        return false;
    }

    let from_shape = world.get_block_state(&from).get_collision_shape();
    let to_shape   = world.get_block_state(&to).get_collision_shape();

    !merged_face_occludes(from_shape, to_shape, direction)
}

/// Checks if a block at the given world position can hold any fluid.
///
/// Delegates to [`can_hold_any_fluid_state`] after reading the block state.
#[must_use]
pub fn can_hold_any_fluid(world: &World, pos: &BlockPos) -> bool {
    let state = world.get_block_state(pos);
    can_hold_any_fluid_state(state)
}

/// Checks if a block state can hold any fluid, without world access.
///
/// Direct equivalent of vanilla's `FlowingFluid.canHoldAnyFluid(BlockState)`:
/// 1. `LiquidBlockContainer` → true  (we approximate via WATERLOGGED property)
/// 2. `blocksMotion()` → false        (we use `has_collision`)
/// 3. Block exclusion list → false     (doors, signs, ladders, etc.)
/// 4. Otherwise → true
#[must_use]
pub fn can_hold_any_fluid_state(state: BlockStateId) -> bool {
    use steel_registry::blocks::properties::BlockStateProperties;

    let block = state.get_block();

    // 1. LiquidBlockContainer equivalent — waterloggable blocks always accept fluid.
    if state.try_get_value(&BlockStateProperties::WATERLOGGED).is_some() {
        return true;
    }

    // 2. blocksMotion() — solid blocks prevent fluid entry.
    if block.config.has_collision {
        return false;
    }

    // 3. Block exclusion list — non-solid blocks that still reject fluid.
    !is_fluid_excluded_block(block)
}

/// Returns true if a block is in the vanilla fluid exclusion list.
///
/// These blocks have no collision (`!blocksMotion()`) but still reject fluid.
/// Matches vanilla `FlowingFluid.canHoldAnyFluid()` lines 401-417:
/// - `block instanceof DoorBlock`  → identified by key suffix `_door`
/// - `blockState.is(BlockTags.SIGNS)` → identified by key containing `sign`
/// - Individual blocks: ladder, sugar cane, bubble column,
///   nether portal, end portal, end gateway, structure void
fn is_fluid_excluded_block(block: BlockRef) -> bool {
    let path: &str = &block.key.path;

    // Vanilla: block instanceof DoorBlock
    // All door blocks end with "_door" (oak_door, iron_door, etc.).
    // Trapdoors end with "_trapdoor" so they are NOT matched.
    if path.ends_with("_door") {
        return true;
    }

    // Vanilla: blockState.is(BlockTags.SIGNS)
    // Covers wall_sign, hanging_sign, wall_hanging_sign, etc.
    if path.contains("sign") {
        return true;
    }

    // Individual block checks (vanilla: blockState.is(Blocks.X))
    std::ptr::eq(block, vanilla_blocks::LADDER)
        || std::ptr::eq(block, vanilla_blocks::SUGAR_CANE)
        || std::ptr::eq(block, vanilla_blocks::BUBBLE_COLUMN)
        || std::ptr::eq(block, vanilla_blocks::NETHER_PORTAL)
        || std::ptr::eq(block, vanilla_blocks::END_PORTAL)
        || std::ptr::eq(block, vanilla_blocks::END_GATEWAY)
        || std::ptr::eq(block, vanilla_blocks::STRUCTURE_VOID)
}

/// Checks if fluid can pass through to a position horizontally.
///
/// This is the world-querying entry point. It reads the block state at `pos`
/// and delegates entirely to [`can_pass_horizontally_internal`], ensuring a
/// single source of truth for the passability logic.
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
/// Single source of truth used by both the world-querying
/// [`can_pass_horizontally`] and [`SpreadContext`] (which supplies a
/// cached `BlockStateId` to avoid redundant world lookups).
///
/// A position is passable when:
/// 1. The block is air or replaceable (trivially open).
/// 2. The block's collision shape is not a full cube (partial shapes like
///    slabs, stairs, fences let fluid through).
/// 3. The block is waterloggable (`WATERLOGGED` property exists).
/// 4. The block already contains the same flowing fluid (lower level).
#[must_use]
pub fn can_pass_horizontally_internal(state: BlockStateId, target_fluid_id: FluidRef) -> bool {
    use steel_registry::blocks::shapes::is_shape_full_block;

    let block = state.get_block();

    // 1. Air and replaceable blocks are always passable.
    if block.config.is_air || block.config.replaceable {
        return true;
    }

    let shape = state.get_collision_shape();

    // 2. Full-cube collision shape → fluid is blocked unless the block is already
    //    the same flowing fluid (e.g. water flowing into a lower water level).
    if is_shape_full_block(shape) {
        let fluid_state = get_fluid_state_from_block(state);
        return std::ptr::eq(fluid_state.fluid_id, target_fluid_id) && !fluid_state.is_source();
    }

    // 3. Empty shape → passable (non-solid decorations, open blocks).
    if shape.is_empty() {
        return true;
    }

    // 4. Waterloggable blocks (LiquidBlockContainer) are always valid spread targets.
    use steel_registry::blocks::properties::BlockStateProperties;
    if state.try_get_value(&BlockStateProperties::WATERLOGGED).is_some() {
        return true;
    }

    // 5. Block already contains the same flowing fluid at a lower level.
    let fluid_state = get_fluid_state_from_block(state);
    if std::ptr::eq(fluid_state.fluid_id, target_fluid_id) && !fluid_state.is_source() {
        return true;
    }

    false
}