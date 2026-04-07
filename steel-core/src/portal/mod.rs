//! Dimension portal system for nether/end portals and future portal types.

use crate::world::World;
use glam::DVec3;
use std::sync::Arc;
use steel_utils::BlockPos;

pub mod portal_shape;

/// Describes a teleport transition to another dimension.
pub struct TeleportTransition {
    /// The target world to teleport into.
    pub target_world: Arc<World>,
    /// The position in the target world.
    pub position: DVec3,
    /// The rotation (yaw, pitch) in the target world.
    pub rotation: (f32, f32),
    /// Portal cooldown in ticks (prevents immediate re-entry).
    pub portal_cooldown: i32,
}

/// A queued request to change an entity's dimension.
pub enum DimensionChangeRequest {
    /// Pre-computed transition (players after chunk pre-warming).
    Computed(TeleportTransition),
    /// Portal position — server computes destination at processing time.
    /// TODO: implement portal destination calculation (`nether_portal::calculate_destination`)
    Portal {
        /// The world the entity is currently in.
        source_world: Arc<World>,
        /// The portal block position.
        portal_pos: BlockPos,
    },
}
