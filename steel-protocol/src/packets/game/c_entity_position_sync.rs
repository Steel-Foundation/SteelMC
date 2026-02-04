use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_ENTITY_POSITION_SYNC;

/// Synchronizes an entity's position, velocity, and rotation.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_ENTITY_POSITION_SYNC)]
pub struct CEntityPositionSync {
    #[write(as = VarInt)]
    pub entity_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub velocity_x: f64,
    pub velocity_y: f64,
    pub velocity_z: f64,
    /// Rotation on the X axis, in degrees
    pub yaw: f32,
    /// Rotation on the Y axis, in degrees
    pub pitch: f32,
    pub on_ground: bool,
}
