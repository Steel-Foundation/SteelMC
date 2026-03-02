//! Core fluid flowing behavior.
//!
//! Based on vanilla's FlowingFluid.java.
//!
// TODO: Consider splitting this file into multiple modules:
//   - fluid_state.rs: FluidState struct and impl
//   - fluid_trait.rs: FluidBehaviour trait
//   - spread_logic.rs: get_spread, get_new_liquid functions
//   - collision.rs: can_pass_through_wall, can_hold_any_fluid
// TODO: Add comprehensive module-level documentation for each public function
// TODO: Add unit tests in tests/ directory

use std::ptr;

use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::shapes::is_shape_full_block;
use steel_registry::fluid::fluid_tags;
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_fluids;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

// get_fluid_state_from_block is defined in this file
use crate::world::World;

use super::spread_context::SpreadContext;

/// Checks if a fluid ID is in the water tag (includes water and `flowing_water`).
/// This matches vanilla's FluidTags.WATER behavior.
#[must_use]
pub fn is_water(fluid_id: FluidRef) -> bool {
    if fluid_id.is_empty {
        return false;
    }
    REGISTRY.fluids.is_in_tag(fluid_id, &fluid_tags::water())
}

/// Checks if a fluid ID is in the lava tag (includes lava and `flowing_lava`).
/// This matches vanilla's FluidTags.LAVA behavior.
#[must_use]
pub fn is_lava(fluid_id: FluidRef) -> bool {
    if fluid_id.is_empty {
        return false;
    }
    REGISTRY.fluids.is_in_tag(fluid_id, &fluid_tags::lava())
}

/// Checks if a fluid state contains water (including flowing water and waterlogged blocks).
#[must_use]
pub fn is_water_state(fluid_state: FluidState) -> bool {
    is_water(fluid_state.fluid_id)
}

/// Checks if a fluid state contains lava (including flowing lava).
#[must_use]
pub fn is_lava_state(fluid_state: FluidState) -> bool {
    is_lava(fluid_state.fluid_id)
}

/// Gets the water source fluid ref from the registry.
#[must_use]
pub fn water_id() -> FluidRef {
    &vanilla_fluids::WATER
}

/// Gets the lava source fluid ref from the registry.
#[must_use]
pub fn lava_id() -> FluidRef {
    &vanilla_fluids::LAVA
}

/// Trait for fluid behavior implementations.
pub trait FluidBehaviour: Send + Sync {
    /// Returns the fluid type ID (0=empty, `1=flowing_water`, 2=water, `3=flowing_lava`, 4=lava).
    fn fluid_type(&self) -> FluidRef;

    /// Returns the tick delay for this fluid.
    fn tick_delay(&self) -> u32;

    /// Returns the drop-off per block.
    fn drop_off(&self) -> u8;

    /// Returns the slope find distance for this fluid.
    fn slope_find_distance(&self) -> u8;

    /// Called when a scheduled tick fires for this fluid.
    fn tick(&self, world: &World, pos: BlockPos, current_tick: u64);

    /// Spreads the fluid from the given position.
    fn spread(&self, world: &World, pos: BlockPos, fluid_state: FluidState, current_tick: u64);

    /// Returns true if this fluid can be replaced by another fluid from a direction.
    /// Based on vanilla's `Fluid.canBeReplacedWith()`.
    ///
    /// # Arguments
    /// * `fluid_state` - The current fluid state at the position
    /// * `world` - The world
    /// * `pos` - The position being checked
    /// * `other_fluid` - The fluid trying to replace this one (ID)
    /// * `direction` - The direction the other fluid is coming from
    fn can_be_replaced_with(
        &self,
        fluid_state: FluidState,
        world: &World,
        pos: BlockPos,
        other_fluid: FluidRef,
        _direction: Direction,
    ) -> bool;
}

/// Gets the fluid state at a block position.
///
/// This derives `FluidState` from `BlockState` (Option A approach for simplicity).
#[must_use]
pub fn get_fluid_state(world: &World, pos: &BlockPos) -> FluidState {
    let state = world.get_block_state(pos);
    get_fluid_state_from_block(state)
}

/// Gets the fluid state from a block state.
#[must_use]
pub fn get_fluid_state_from_block(state: BlockStateId) -> FluidState {
    let block = state.get_block();

    if ptr::eq(block, vanilla_blocks::WATER) {
        let level: u8 = state
            .try_get_value(&BlockStateProperties::LEVEL)
            .unwrap_or(0);
        FluidState::from_block_level(water_id(), level)
    } else if ptr::eq(block, vanilla_blocks::LAVA) {
        let level: u8 = state
            .try_get_value(&BlockStateProperties::LEVEL)
            .unwrap_or(0);
        FluidState::from_block_level(lava_id(), level)
    } else {
        // Check waterlogged property
        if let Some(true) = state.try_get_value(&BlockStateProperties::WATERLOGGED) {
            FluidState::source(water_id())
        } else {
            FluidState::EMPTY
        }
    }
}

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
    use steel_registry::blocks::shapes::is_shape_full_block;

    if !world.is_in_valid_bounds(&to) {
        return false;
    }

    let from_state = world.get_block_state(&from);
    let to_state = world.get_block_state(&to);

    // Get collision shapes
    let from_shape = from_state.get_collision_shape();
    let to_shape = to_state.get_collision_shape();

    // If both shapes are full blocks, fluid cannot pass
    if is_shape_full_block(from_shape) && is_shape_full_block(to_shape) {
        return false;
    }

    // If either shape is empty, fluid can pass
    if from_shape.is_empty() || to_shape.is_empty() {
        return true;
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

    // TODO: Implement proper face occlusion check using Shapes.mergedFaceOccludes
    // This requires checking if the faces between the two blocks are occluded
    // For now, we use a simplified check based on collision
    !to_block.config.has_collision
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

    // TODO: Check for LiquidBlockContainer trait when implemented
    // Blocks like cauldrons, waterlogged blocks, etc. can hold fluid
    // if block.is_liquid_container() {
    //     return true;
    // }

    // Check if block blocks motion (solid blocks that don't allow fluids)
    // TODO: This check should be more sophisticated - we need to check if the
    // block implements DoorBlock, SignBlock, etc.
    // For now, we use a simplified approach based on config flags

    // TODO: Check block tags when properly implemented:
    // - BlockTags.DOORS
    // - BlockTags.ALL_SIGNS
    // - LADDER
    // - SUGAR_CANE
    // - BUBBLE_COLUMN
    // - NETHER_PORTAL
    // - END_PORTAL
    // - END_GATEWAY
    // - STRUCTURE_VOID

    // Simplified check: air and replaceable blocks can hold fluid
    // Non-replaceable blocks with collision generally cannot
    if block.config.is_air || block.config.replaceable {
        return true;
    }

    // If block has collision and is not replaceable, it likely can't hold fluid
    // TODO: Add specific block type checks here when those block types are implemented
    !block.config.has_collision
}

/// Calculates the new fluid state for a position based on neighbors.
/// This is vanilla's `getNewLiquid()` function.
///
/// Returns the fluid state that should exist at this position.
/// A fluid block can only be supported by:
/// - A source block nearby
/// - Water directly above (creates falling water with level 8)
/// - A neighbor with HIGHER amount (lower level) that can flow into this position
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

    log::info!("get_new_liquid: pos={:?}, fluid_id={}, source_count={}, max_incoming_amount={}", pos, fluid_id.key, source_count, max_incoming_amount);
    // Water source conversion: 2+ adjacent sources + solid below = new source
    // Check game rule for water source conversion (vanilla: default true)
    if is_water(fluid_id) && source_count >= 2 {
        use steel_registry::vanilla_game_rules::WATER_SOURCE_CONVERSION;
        let can_convert = match world.get_game_rule(WATER_SOURCE_CONVERSION) {
            GameRuleValue::Bool(val) => val,
            GameRuleValue::Int(_) => true, // Default to true if game rule not found
        };

        log::info!("WATER SOURCE CONVERSION: can_convert={}", can_convert);
        if can_convert {
            let below_pos = pos.offset(0, -1, 0);
            let below_state = world.get_block_state(&below_pos);
            let below_block = below_state.get_block();
            let below_fluid = get_fluid_state_from_block(below_state);
            // Solid block OR source of same type below
            if (!below_block.config.replaceable && !below_block.config.is_air)
                || below_fluid.is_source()
            {
                log::info!("WATER SOURCE CONVERSION: block_name={}, repl={}, air={}, is_source={}", below_block.key, below_block.config.replaceable, below_block.config.is_air, below_fluid.is_source());
                return FluidState::source(water_id());
            } else {
                log::info!("WATER SOURCE CONVERSION failed due to block below: block_name={}, repl={}, air={}, is_source={}", below_block.key, below_block.config.replaceable, below_block.config.is_air, below_fluid.is_source());
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

/// Converts a `FluidState` to a `BlockStateId` for the corresponding fluid block.
///
/// Block state LEVEL property:
/// - 0 = source
/// - 1-7 = flowing water (1 = most, 7 = least)
/// - 8 = falling water (from above)
#[must_use]
pub fn fluid_state_to_block(fluid_state: FluidState) -> BlockStateId {
    let fluid_id = fluid_state.fluid_id;
    if fluid_id.is_empty {
        REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR)
    } else if is_water(fluid_id) {
        let base = REGISTRY.blocks.get_default_state_id(vanilla_blocks::WATER);
        // Use FluidState's to_block_level method for proper conversion
        let level = fluid_state.to_block_level();
        base.set_value(&BlockStateProperties::LEVEL, level)
    } else if is_lava(fluid_id) {
        let base = REGISTRY.blocks.get_default_state_id(vanilla_blocks::LAVA);
        let level = fluid_state.to_block_level();
        base.set_value(&BlockStateProperties::LEVEL, level)
    } else {
        // Unknown fluid type - default to air
        REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR)
    }
}

// ============================================================================
// SLOPE FINDING ALGORITHM
// ============================================================================

/// Checks if the given position is a "hole" where water can fall down.
/// A hole is when the block below is air/replaceable or is the same fluid type.
#[must_use]
pub fn is_hole(world: &World, pos: &BlockPos, fluid_id: FluidRef) -> bool {
    let below = pos.offset(0, -1, 0);

    if !world.is_in_valid_bounds(&below) {
        return false;
    }

    let below_state = world.get_block_state(&below);
    let below_block = below_state.get_block();

    // Check if we can flow down
    if below_block.config.is_air || below_block.config.replaceable {
        return true;
    }

    // Check if below is same fluid (water can flow into water)
    let below_fluid = get_fluid_state_from_block(below_state);
    if std::ptr::eq(below_fluid.fluid_id, fluid_id) && !below_fluid.is_source() {
        return true;
    }

    false
}

/// Checks if fluid can pass through to a position horizontally.
/// Based on vanilla's path checking for slope finding.
#[must_use]
fn can_pass_horizontally(world: &World, pos: &BlockPos, target_fluid_id: FluidRef) -> bool {
    use steel_registry::blocks::shapes::is_shape_full_block;

    if !world.is_in_valid_bounds(pos) {
        return false;
    }

    let state = world.get_block_state(pos);
    let block = state.get_block();

    // Check collision shape
    let shape = state.get_collision_shape();

    // If shape is a full block, can't pass through
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

/// Context-aware version of `get_slope_distance` for use with `SpreadContext`.
/// Avoids redundant world lookups by caching block states and hole checks.
fn get_slope_distance_with_context(
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
            let distance = get_slope_distance_with_context(
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
        if !can_pass_horizontally(world, &neighbor, fluid_id) {
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
            get_slope_distance_with_context(
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
