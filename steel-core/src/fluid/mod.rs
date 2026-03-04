//! Fluid behavior system.
//!
//! This module handles fluid mechanics: spreading, flowing, waterlogging.
//! Based on vanilla Minecraft's `FlowingFluid` system.
//!
//! ## Fluid System Status (Vanilla Parity)
//! We are striving for 1:1 parity with Java `FlowingFluid` (see `FLUID_REVIEW.md` for background).
//! 
//! ### Implemented ✅
//! - Basic spread mechanics (getNewLiquid, getSpread, slope finding)
//! - Source conversion (2+ sources + solid below)
//! - Game rule support (waterSourceConversion, lavaSourceConversion)
//! - Bucket place/pickup mechanics
//! - Basic Collision checking with VoxelShapes (`can_pass_through_wall` using `merged_face_occludes`)
//! - Basic Waterlogging (preserves `WATERLOGGED` block states when fluids flow down/horizontally)
//! 
//! ### TODO: PARITY ❌ (What's Missing)
//! - TODO: PARITY: Full `LiquidBlockContainer` API (so blocks like stairs/slabs can dynamically `canPlaceLiquid` / `placeLiquid`).
//! - TODO: PARITY: Block Exclusions in `can_hold_any_fluid` (prevent flow through doors, signs, ladders, sugar canes, gates).
//! - TODO: PARITY: Lava/Water Chemistry (generate Obsidian, Cobblestone, Stone when fluids mix).
//! - TODO: PARITY: Tick Scheduling (Fluids need to be scheduled on a tick list like Vanilla, not just immediate arbitrary updates).
//! - TODO: PARITY: Dimension-based Lava Flow (Lava flows 8 blocks in Nether, 4 in Overworld, with different tick speeds).
//! - TODO: PARITY: Sound & Particle Events (Fizzing sounds, bubbling particles).
//! - TODO: PARITY: Entity Interactions (pushing, drowning, extinguishing, lava damage).
//! - TODO: PARITY: Fluid Tags (`minecraft:water`, `minecraft:lava`).
//!
//! ### Issues ⚠️
//! - Bucket stacks cause deadlocks (disabled)
//! - Visual sync issues with infinite sources
pub mod collision;
pub mod conversion;
pub mod flowing_fluid;
pub mod fluid_behavior;
pub mod fluids;
pub mod spread_context;
pub mod state;

// Re-export fluid types from steel_registry
pub use steel_registry::fluid::fluid_tags;
pub use steel_registry::fluid::{Fluid, FluidRef, FluidState};

// Re-export specific structs/functions
pub use fluids::{EmptyFluid, LavaFluid, WaterFluid};
pub use fluid_behavior::FluidBehavior;
pub use flowing_fluid::FlowingFluidBehavior;

// Re-export utility functions from their respective modules
pub use collision::{can_hold_any_fluid, can_pass_through_wall};
pub use state::{
    fluid_state_to_block, get_fluid_state, get_fluid_state_from_block, 
    is_lava, is_lava_state, is_water, is_water_state, lava_id, water_id,
};
pub use conversion::{get_new_liquid, get_spread, is_hole};
