//! Fluid state <-> block state conversions.
//!
//! Responsible for deriving `FluidState` from `BlockState`
//! and converting `FluidState` back into `BlockStateId`.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::fluid::{FluidRef, FluidState, is_lava_fluid, is_water_fluid};
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::{BlockStateBehaviorExt as _, FLUID_BEHAVIORS};
use crate::world::World;
use steel_registry::vanilla_fluids;

const FLOW_BELOW_HEIGHT_OFFSET: f32 = 0.888_888_9;
const FALLING_FLOW_DOWNWARD: f64 = -6.0;

/// Gets the fluid state at a given position.
///
/// Derives `FluidState` from the block state.
#[must_use]
pub fn get_fluid_state(world: &Arc<World>, pos: BlockPos) -> FluidState {
    let state = world.get_block_state(pos);
    get_fluid_state_from_block(state)
}

/// Gets the fluid state from a raw `BlockStateId`.
#[must_use]
pub fn get_fluid_state_from_block(state: BlockStateId) -> FluidState {
    state.get_fluid_state()
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
        return REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    }

    if is_water_fluid(fluid_id) {
        if fluid_id == &vanilla_fluids::WATER
            && fluid_state.is_source()
            && existing_state
                .try_get_value(&BlockStateProperties::WATERLOGGED)
                .is_some()
        {
            // Vanilla SimpleWaterloggedBlock only accepts source WATER, not FLOWING_WATER.
            return existing_state.set_value(&BlockStateProperties::WATERLOGGED, true);
        }

        // If not waterloggable, fall back to pure water block
        let base = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
        let level = fluid_state.to_block_level();
        return base.set_value(&BlockStateProperties::LEVEL, level);
    }

    if is_lava_fluid(fluid_id) {
        let base = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::LAVA);
        let level = fluid_state.to_block_level();
        return base.set_value(&BlockStateProperties::LEVEL, level);
    }

    // Unknown fluid type - default to air
    REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
}

/// Converts a `FluidState` into a `BlockStateId` directly without preserving any block.
///
/// Handles LEVEL property mapping.
#[must_use]
pub fn fluid_state_to_block(fluid_state: FluidState) -> BlockStateId {
    let fluid_id = fluid_state.fluid_id;
    if fluid_id.is_empty {
        REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
    } else if is_water_fluid(fluid_id) {
        let base = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
        // Use FluidState's to_block_level method for proper conversion
        let level = fluid_state.to_block_level();
        base.set_value(&BlockStateProperties::LEVEL, level)
    } else if is_lava_fluid(fluid_id) {
        let base = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::LAVA);
        let level = fluid_state.to_block_level();
        base.set_value(&BlockStateProperties::LEVEL, level)
    } else {
        // Unknown fluid type - default to air
        REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
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
pub fn get_height(world: &Arc<World>, pos: BlockPos, fluid_state: FluidState) -> f32 {
    if fluid_state.is_empty() {
        return 0.0;
    }

    let above = pos.offset(0, 1, 0);
    let above_fluid = get_fluid_state(world, above);
    let behavior = FLUID_BEHAVIORS.get_behavior(fluid_state.fluid_id);
    get_height_with(fluid_state, above_fluid, |candidate| {
        behavior.is_same(candidate.fluid_id)
    })
}

fn get_height_with<S>(fluid_state: FluidState, above_fluid: FluidState, same_fluid: S) -> f32
where
    S: Fn(FluidState) -> bool,
{
    if fluid_state.is_empty() {
        return 0.0;
    }

    if same_fluid(above_fluid) {
        1.0
    } else {
        get_own_height(fluid_state)
    }
}

/// Returns vanilla `FlowingFluid.getFlow` for this fluid state.
#[must_use]
pub fn get_flow(world: &Arc<World>, pos: BlockPos, fluid_state: FluidState) -> DVec3 {
    if fluid_state.is_empty() {
        return DVec3::ZERO;
    }

    let behavior = FLUID_BEHAVIORS.get_behavior(fluid_state.fluid_id);
    get_flow_with(
        pos,
        fluid_state,
        |candidate| behavior.is_same(candidate.fluid_id),
        |fluid_pos| get_fluid_state(world, fluid_pos),
        |block_pos| world.get_block_state(block_pos),
    )
}

fn get_flow_with<S, F, B>(
    pos: BlockPos,
    fluid_state: FluidState,
    same_fluid: S,
    mut fluid_at: F,
    mut block_at: B,
) -> DVec3
where
    S: Fn(FluidState) -> bool,
    F: FnMut(BlockPos) -> FluidState,
    B: FnMut(BlockPos) -> BlockStateId,
{
    if fluid_state.is_empty() {
        return DVec3::ZERO;
    }

    let own_height = get_own_height(fluid_state);
    let mut flow = DVec3::ZERO;
    for direction in Direction::HORIZONTAL {
        let neighbor_pos = direction.relative(pos);
        let neighbor_fluid = fluid_at(neighbor_pos);
        if !affects_flow_with(neighbor_fluid, &same_fluid) {
            continue;
        }

        let mut neighbor_height = get_own_height(neighbor_fluid);
        let mut distance = 0.0;
        if neighbor_height == 0.0 {
            if !block_at(neighbor_pos).blocks_motion() {
                let below_fluid = fluid_at(neighbor_pos.below());
                if affects_flow_with(below_fluid, &same_fluid) {
                    neighbor_height = get_own_height(below_fluid);
                    if neighbor_height > 0.0 {
                        distance = own_height - (neighbor_height - FLOW_BELOW_HEIGHT_OFFSET);
                    }
                }
            }
        } else {
            distance = own_height - neighbor_height;
        }

        if distance != 0.0 {
            let (dx, dz) = direction.offset_xz();
            flow.x += f64::from(dx) * f64::from(distance);
            flow.z += f64::from(dz) * f64::from(distance);
        }
    }

    if fluid_state.falling {
        for direction in Direction::HORIZONTAL {
            let neighbor_pos = direction.relative(pos);
            if is_solid_face_with(
                neighbor_pos,
                direction,
                &same_fluid,
                &mut fluid_at,
                &mut block_at,
            ) || is_solid_face_with(
                neighbor_pos.above(),
                direction,
                &same_fluid,
                &mut fluid_at,
                &mut block_at,
            ) {
                flow = flow.normalize_or_zero() + DVec3::new(0.0, FALLING_FLOW_DOWNWARD, 0.0);
                break;
            }
        }
    }

    flow.normalize_or_zero()
}

fn affects_flow_with<S>(neighbor_fluid: FluidState, same_fluid: &S) -> bool
where
    S: Fn(FluidState) -> bool,
{
    neighbor_fluid.is_empty() || same_fluid(neighbor_fluid)
}

fn is_solid_face_with<S, F, B>(
    pos: BlockPos,
    direction: Direction,
    same_fluid: &S,
    fluid_at: &mut F,
    block_at: &mut B,
) -> bool
where
    S: Fn(FluidState) -> bool,
    F: FnMut(BlockPos) -> FluidState,
    B: FnMut(BlockPos) -> BlockStateId,
{
    let state = block_at(pos);
    let fluid_state = fluid_at(pos);
    if same_fluid(fluid_state) {
        return false;
    }
    if direction == Direction::Up {
        return true;
    }
    state.get_block() != &vanilla_blocks::ICE && state.is_face_sturdy_at(pos, direction)
}

#[cfg(test)]
mod tests {
    use steel_registry::fluid::FluidStateExt as _;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks, vanilla_fluids};

    use super::*;

    fn same_water(candidate: FluidState) -> bool {
        candidate.is_water()
    }

    #[test]
    fn height_treats_source_and_flowing_variants_as_same_fluid_above() {
        init_test_registry();

        assert_eq!(
            get_height_with(
                FluidState::source(&vanilla_fluids::WATER),
                FluidState::flowing(&vanilla_fluids::FLOWING_WATER, 4, false),
                same_water,
            )
            .to_bits(),
            1.0_f32.to_bits()
        );
        assert_eq!(
            get_height_with(
                FluidState::flowing(&vanilla_fluids::FLOWING_WATER, 4, false),
                FluidState::source(&vanilla_fluids::WATER),
                same_water,
            )
            .to_bits(),
            1.0_f32.to_bits()
        );
    }

    #[test]
    fn empty_fluid_height_is_zero() {
        init_test_registry();

        assert_eq!(
            get_height_with(
                FluidState::EMPTY,
                FluidState::source(&vanilla_fluids::WATER),
                same_water,
            )
            .to_bits(),
            0.0_f32.to_bits()
        );
    }

    #[test]
    fn flow_points_toward_lower_same_fluid_neighbor() {
        init_test_registry();
        let pos = BlockPos::new(0, 64, 0);
        let flow = get_flow_with(
            pos,
            FluidState::source(&vanilla_fluids::WATER),
            same_water,
            |fluid_pos| {
                if fluid_pos == pos.east() {
                    FluidState::flowing(&vanilla_fluids::FLOWING_WATER, 4, false)
                } else {
                    FluidState::EMPTY
                }
            },
            |_block_pos| vanilla_blocks::AIR.default_state(),
        );

        assert!((flow.x - 1.0).abs() < f64::EPSILON);
        assert!(flow.y.abs() < f64::EPSILON);
        assert!(flow.z.abs() < f64::EPSILON);
    }

    #[test]
    fn falling_flow_pulls_down_when_horizontal_neighbor_has_solid_face() {
        init_test_registry();
        let pos = BlockPos::new(0, 64, 0);
        let flow = get_flow_with(
            pos,
            FluidState::flowing(&vanilla_fluids::FLOWING_WATER, 8, true),
            same_water,
            |_fluid_pos| FluidState::EMPTY,
            |block_pos| {
                if block_pos == pos.east() {
                    vanilla_blocks::STONE.default_state()
                } else {
                    vanilla_blocks::AIR.default_state()
                }
            },
        );

        assert!(flow.x.abs() < f64::EPSILON);
        assert!((flow.y + 1.0).abs() < f64::EPSILON);
        assert!(flow.z.abs() < f64::EPSILON);
    }
}
