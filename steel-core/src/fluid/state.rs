//! Fluid state <-> block state conversions.
//!
//! Responsible for deriving `FluidState` from `BlockState`
//! and converting `FluidState` back into `BlockStateId`.

use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::fluid::{FluidRef, FluidState, is_lava_fluid, is_water_fluid};
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

use crate::world::World;
use steel_registry::vanilla_fluids;

/// Gets the fluid state at a given position.
///
/// Derives `FluidState` from the block state.
#[must_use]
pub fn get_fluid_state(world: &World, pos: &BlockPos) -> FluidState {
    let state = world.get_block_state(pos);
    get_fluid_state_from_block(state)
}

/// Gets the fluid state from a raw `BlockStateId`.
#[must_use]
pub fn get_fluid_state_from_block(state: BlockStateId) -> FluidState {
    let block = state.get_block();

    if block == vanilla_blocks::WATER {
        let level: u8 = state
            .try_get_value(&BlockStateProperties::LEVEL)
            .unwrap_or(0);
        FluidState::from_block_level(water_id(), level)
    } else if block == vanilla_blocks::LAVA {
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

/// Converts a `FluidState` into a `BlockStateId`, preserving the identity of an existing block.
///
/// If `existing_state` is a waterloggable block, this sets or clears its WATERLOGGED
/// property rather than replacing the block entirely. Otherwise it falls back to the
/// raw fluid block (WATER/LAVA) or AIR for empty fluid.
#[must_use]
pub fn fluid_state_to_block_with_existing(
    fluid_state: FluidState,
    existing_state: BlockStateId,
) -> BlockStateId {
    let fluid_id = fluid_state.fluid_id;
    if fluid_id.is_empty {
        // If empty, and the existing block can be waterlogged, un-waterlog it.
        // If it cannot be waterlogged, it becomes air.
        if existing_state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
        {
            return existing_state.set_value(&BlockStateProperties::WATERLOGGED, false);
        }
        return REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
    }

    // If it's water, check if the block can be waterlogged.
    // Vanilla's FlowingFluid.spreadTo() calls LiquidBlockContainer.placeLiquid()
    // for any fluid level (source or flowing), so we waterlog regardless of amount.
    if is_water_fluid(fluid_id) {
        if existing_state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
        {
            return existing_state.set_value(&BlockStateProperties::WATERLOGGED, true);
        }

        // If not waterloggable, fall back to pure water block
        let base = REGISTRY.blocks.get_default_state_id(vanilla_blocks::WATER);
        let level = fluid_state.to_block_level();
        return base.set_value(&BlockStateProperties::LEVEL, level);
    }

    if is_lava_fluid(fluid_id) {
        let base = REGISTRY.blocks.get_default_state_id(vanilla_blocks::LAVA);
        let level = fluid_state.to_block_level();
        return base.set_value(&BlockStateProperties::LEVEL, level);
    }

    // Unknown fluid type - default to air
    REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR)
}

/// Converts a `FluidState` into a `BlockStateId` directly without preserving any block.
///
/// Handles LEVEL property mapping.
#[must_use]
pub fn fluid_state_to_block(fluid_state: FluidState) -> BlockStateId {
    let fluid_id = fluid_state.fluid_id;
    if fluid_id.is_empty {
        REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR)
    } else if is_water_fluid(fluid_id) {
        let base = REGISTRY.blocks.get_default_state_id(vanilla_blocks::WATER);
        // Use FluidState's to_block_level method for proper conversion
        let level = fluid_state.to_block_level();
        base.set_value(&BlockStateProperties::LEVEL, level)
    } else if is_lava_fluid(fluid_id) {
        let base = REGISTRY.blocks.get_default_state_id(vanilla_blocks::LAVA);
        let level = fluid_state.to_block_level();
        base.set_value(&BlockStateProperties::LEVEL, level)
    } else {
        // Unknown fluid type - default to air
        REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR)
    }
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

/// Returns the fluid's own height as a fraction of a full block.
/// `amount / 9.0` — source blocks have `amount = 8`, giving `0.888..`.
/// Flowing blocks range from `amount = 1` (thin) to `7` (tall).
#[must_use]
pub fn get_own_height(fluid_state: FluidState) -> f32 {
    f32::from(fluid_state.amount) / 9.0
}

/// Returns the effective fluid height at a position, accounting for fluid above.
/// If the same fluid type occupies the block directly above (`hasSameAbove`),
/// the height is `1.0` (full block). Otherwise it is `get_own_height(fluid_state)`.
#[must_use]
pub fn get_height(world: &World, pos: &BlockPos, fluid_state: FluidState) -> f32 {
    let above = pos.offset(0, 1, 0);
    let above_fluid = get_fluid_state(world, &above);
    if above_fluid.fluid_id == fluid_state.fluid_id {
        1.0
    } else {
        get_own_height(fluid_state)
    }
}
