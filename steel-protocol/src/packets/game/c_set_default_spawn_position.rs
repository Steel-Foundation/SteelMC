//! Clientbound default spawn position packet.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_DEFAULT_SPAWN_POSITION;
use steel_utils::GlobalPos;

/// Updates the client's default world spawn marker.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_DEFAULT_SPAWN_POSITION)]
pub struct CSetDefaultSpawnPosition {
    /// Dimension and block position of the default spawn.
    pub global_pos: GlobalPos,
    /// Spawn yaw.
    pub yaw: f32,
    /// Spawn pitch.
    pub pitch: f32,
}
