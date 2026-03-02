//! Fluid behavior system.
//!
//! This module handles fluid mechanics: spreading, flowing, waterlogging.
//! Based on vanilla Minecraft's `FlowingFluid` system.
//!
//! ## Current Status
//! See `FLUID_REVIEW.md` for comprehensive vanilla parity analysis.
//!
//! ### Implemented ✅
//! - Basic spread mechanics (getNewLiquid, getSpread, slope finding)
//! - Source conversion (2+ sources + solid below)
//! - Game rule support (waterSourceConversion, lavaSourceConversion)
//! - Bucket place/pickup mechanics
//!
//! ### Missing ❌
//! - Lava-water chemistry (obsidian/cobblestone)
//! - Collision shape checks (canPassThroughWall with `VoxelShape`)
//! - Block type exclusions (doors, signs, ladders)
//! - Waterlogging support
//! - Dimension-based lava speed
//! - Sound and particle effects
//! - Entity interactions (damage, extinguishing)
//!
//! ### Issues ⚠️
//! - Bucket stacks cause deadlocks (disabled)
//! - Visual sync issues with infinite sources
//!
//! TODO: Add `FluidProperties` module when block properties system supports fluid-specific properties
//! TODO: Add `FluidTags` module for fluid tag support (e.g., minecraft:water, minecraft:lava)
//! TODO: Consider organizing fluids into submodules by category (vanilla, modded)

mod empty;
pub mod flowing;
mod lava;
pub mod spread_context;
mod water;

// Re-export fluid types from steel_registry
pub use steel_registry::fluid::fluid_tags;
pub use steel_registry::fluid::{Fluid, FluidRef, FluidState};

pub use empty::EmptyFluid;
pub use flowing::{
    FluidBehaviour, can_hold_any_fluid, can_pass_through_wall, fluid_state_to_block,
    get_fluid_state, get_fluid_state_from_block, get_new_liquid, get_spread, is_hole, is_lava,
    is_lava_state, is_water, is_water_state, lava_id, water_id,
};
pub use lava::LavaFluid;
pub use water::WaterFluid;
