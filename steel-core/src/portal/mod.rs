//! World portal system for nether/end portals and future portal types.
//!
//! Vanilla commonly calls loaded worlds "dimensions". Steel uses "world" for
//! loaded runtime worlds and reserves "dimension type" for the vanilla registry
//! entry that defines world rules.

use crate::world::World;
use glam::DVec3;
use std::sync::Arc;
use steel_utils::BlockPos;

pub mod portal_shape;

/// Describes a teleport transition to another loaded world.
///
/// Vanilla names loaded worlds "dimensions" in packets and saves. Steel uses
/// "world" for runtime loaded world instances, reserving "dimension type" for
/// the vanilla registry entry that defines height, skylight, ceiling, etc.
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

/// A queued request to move an entity between loaded worlds.
///
/// Vanilla calls these world changes "dimension changes". Steel keeps the
/// runtime API named after loaded worlds to avoid confusing worlds with vanilla
/// dimension types.
pub enum WorldChangeRequest {
    /// Pre-computed transition (players after chunk pre-warming).
    Computed(TeleportTransition),
    /// Command-driven world change to the target world's spawn.
    WorldSpawn {
        /// The target world to teleport into.
        target_world: Arc<World>,
    },
    /// Portal position — server computes destination at processing time.
    /// TODO: implement portal destination calculation (`nether_portal::calculate_destination`)
    Portal {
        /// The world the entity is currently in.
        source_world: Arc<World>,
        /// The portal block position.
        portal_pos: BlockPos,
    },
}
