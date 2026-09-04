use glam::DVec3;
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_MOVE_MINECART_ALONG_TRACK;

/// Clientbound move minecart along track packet.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_MOVE_MINECART_ALONG_TRACK)]
pub struct CMoveMinecartAlongTrack {
    #[write(as = VarInt)]
    pub entity_id: i32,
    pub steps: Vec<MinecartStep>,
}

/// A single step of minecart movement.
#[derive(WriteTo, Clone, Debug)]
pub struct MinecartStep {
    pub position: DVec3,
    pub movement: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub weight: f32,
}

impl CMoveMinecartAlongTrack {
    /// Creates a new `CMoveMinecartAlongTrack` packet.
    #[must_use]
    pub const fn new(entity_id: i32, steps: Vec<MinecartStep>) -> Self {
        Self { entity_id, steps }
    }
}
