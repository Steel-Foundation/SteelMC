//! Physics engine for entity movement with vanilla Minecraft parity.
//!
//! This module implements the core physics simulation for moving entities through
//! the world with proper collision detection, including:
//! - Step-up mechanics (climbing blocks ≤ `max_up_step` height)
//! - Sneak-edge prevention (staying on block edges while crouching)
//! - VoxelShape-based collision using AABB lists
//!
//! The implementation closely follows vanilla's `Entity.move()` method to ensure
//! 1:1 movement validation for anti-cheat purposes.

use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_blocks,
};
use steel_utils::{BlockStateId, Identifier};

pub mod collision;
pub(crate) mod entity_move;
pub mod movement_validation;
pub(crate) mod physics_state;
pub mod shapes;

// Public API
pub use collision::{
    CollisionWorld, WorldCollisionProvider, has_block_collision, has_collision,
    is_colliding_with_new_shapes,
};
pub(crate) use entity_move::move_entity;
pub use entity_move::{MoveResult, MoverType};
pub(crate) use movement_validation::ClientAuthoredMovementState;
pub use movement_validation::{
    MOVEMENT_ERROR_THRESHOLD, MovementCollisionValidation, movement_error_delta,
};
pub(crate) use physics_state::EntityPhysicsState;
pub use shapes::{collide, join_is_not_empty, merged_face_occludes, translate_shape};

/// Collision epsilon used for AABB deflation (vanilla constant).
pub const COLLISION_EPSILON: f64 = 1.0e-5;

/// Returns whether this block's collision behavior is owned by an extensible namespace.
pub(crate) fn block_has_extensible_collision_behavior(block: BlockRef) -> bool {
    block.key.namespace != Identifier::VANILLA_NAMESPACE
}

fn block_may_expand_collision_cursor(block: BlockRef, has_large_static_shape: bool) -> bool {
    block_has_extensible_collision_behavior(block)
        || block.config.dynamic_shape
        || block == &vanilla_blocks::MOVING_PISTON
        || has_large_static_shape
}

/// Returns whether this state's collision behavior requires live reads around callbacks.
pub(crate) fn block_state_has_extensible_collision_behavior(state: BlockStateId) -> bool {
    block_has_extensible_collision_behavior(state.get_block())
}

/// Returns whether a state may require Vanilla's expanded block-collision cursor.
pub(crate) fn block_state_may_expand_collision_cursor(state: BlockStateId) -> bool {
    block_may_expand_collision_cursor(
        state.get_block(),
        state
            .get_static_collision_shape()
            .has_large_collision_shape(),
    )
}
