//! Physics engine for entity movement with vanilla Minecraft 26.1 parity.
//!
//! This module implements the core physics simulation for moving entities through
//! the world with proper collision detection, including:
//! - Step-up mechanics (climbing blocks ≤ `max_up_step` height)
//! - Sneak-edge prevention (staying on block edges while crouching)
//! - VoxelShape-based collision using AABB lists
//!
//! The implementation closely follows vanilla's `Entity.move()` method to ensure
//! 1:1 movement validation for anti-cheat purposes.

pub mod collision;
pub(crate) mod entity_move;
pub(crate) mod physics_state;
pub mod shapes;

// Public API
pub use collision::{
    CollisionWorld, WorldCollisionProvider, has_block_collision, is_colliding_with_new_blocks,
};
pub(crate) use entity_move::move_entity;
pub use entity_move::{MoveResult, MoverType};
pub(crate) use physics_state::EntityPhysicsState;
pub use shapes::{collide, join_is_not_empty, merged_face_occludes, translate_shape};

/// Collision epsilon used for AABB deflation (vanilla constant).
pub const COLLISION_EPSILON: f64 = 1.0e-5;

/// Movement error threshold for anti-cheat validation (squared distance).
/// Vanilla uses 0.0625 (1/16 block squared).
pub const MOVEMENT_ERROR_THRESHOLD: f64 = 0.0625;

/// Y-axis tolerance value used by vanilla's movement-error branch.
///
/// Vanilla currently uses `yDist > -0.5 || yDist < 0.5`, which zeroes every
/// finite Y residual before the moved-wrongly check.
pub const Y_TOLERANCE: f64 = 0.5;
