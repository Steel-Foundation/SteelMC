//! Fluid behavior system.
//!
//! This module handles fluid mechanics: spreading, flowing, waterlogging.
//! Based on vanilla Minecraft's `FlowingFluid` system.
//!
//! ### TODOs
//! - TODO: Ambient tick dispatcher — `animate_tick` (sounds, particles) needs a client-side `Level.animateTick` equivalent firing at render rate for nearby blocks.
//! - TODO: Particle Events (underwater bubbles, lava pops, drip particles — needs `CLevelParticles` packet).
//! - TODO: Entity Interactions (pushing, drowning, extinguishing, lava damage).
//! - TODO: Block item drops when water destroys blocks (cactus infrastructure merged, needs implementation).
//! - TODO: Lava random tick fire spread.
pub mod collision;
pub mod conversion;
pub mod flowing_fluid;
pub mod fluid_behavior;
pub mod fluids;
mod spread_context;
pub mod state;

// Re-export fluid types from steel_registry
pub use steel_registry::fluid::{
    Fluid, FluidRef, FluidState, FluidStateExt, is_lava_fluid, is_water_fluid,
};

// Re-export specific structs/functions
pub use flowing_fluid::FlowingFluid;
pub use fluid_behavior::FluidBehavior;
pub use fluids::{EmptyFluid, LavaFluid, WaterFluid};

// Re-export utility functions from their respective modules
pub use collision::{
    can_hold_any_fluid, can_hold_any_fluid_state, can_hold_fluid, can_hold_specific_fluid,
    can_pass_through_wall,
};
pub use conversion::{get_new_liquid, get_spread, is_hole};
pub use state::{
    fluid_state_to_block, fluid_state_to_block_with_existing, get_fluid_state,
    get_fluid_state_from_block, get_height, get_own_height, lava_id, water_id,
};
