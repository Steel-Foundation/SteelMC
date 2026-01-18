//! Physics engine for entity movement with vanilla Minecraft 1.21.11 parity.
//!
//! This module implements the core physics simulation for moving entities through
//! the world with proper collision detection, including:
//! - Step-up mechanics (climbing blocks ≤ `max_up_step` height)
//! - Sneak-edge prevention (staying on block edges while crouching)
//! - VoxelShape-based collision using AABB lists
//!
//! The implementation closely follows vanilla's `Entity.move()` method to ensure
//! 1:1 movement validation for anti-cheat purposes.

mod collision;
mod entity_move;
mod physics_state;
mod shapes;

// Public API
pub use collision::{CollisionWorld, WorldCollisionProvider};
pub use entity_move::{MoveResult, MoverType, move_entity};
pub use physics_state::EntityPhysicsState;
pub use shapes::{collide, join_is_not_empty, translate_shape};

/// Collision epsilon used for AABB deflation (vanilla constant).
pub const COLLISION_EPSILON: f64 = 1.0e-5;

/// Movement error threshold for anti-cheat validation (squared distance).
/// Vanilla uses 0.0625 (1/16 block squared).
pub const MOVEMENT_ERROR_THRESHOLD: f64 = 0.0625;

/// Y-axis tolerance for movement validation.
/// Vanilla ignores Y differences within ±0.5 blocks after physics simulation.
pub const Y_TOLERANCE: f64 = 0.5;
