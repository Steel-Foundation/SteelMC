//! Dimension portal system for nether portals and future portal types.

pub mod nether_portal;
pub mod portal_forcer;
pub mod portal_processor;
pub mod portal_shape;

use std::sync::Arc;

use steel_utils::math::Vector3;

use crate::world::World;

/// Describes a teleport transition to another dimension.
pub struct TeleportTransition {
    /// The target world to teleport into.
    pub target_world: Arc<World>,
    /// The position in the target world.
    pub position: Vector3<f64>,
    /// The rotation (yaw, pitch) in the target world.
    pub rotation: (f32, f32),
    /// Portal cooldown in ticks (prevents immediate re-entry).
    pub portal_cooldown: i32,
}
