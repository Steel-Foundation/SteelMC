use glam::DVec3;
use steel_macros::{ReadFrom, ServerPacket};

/// Serverbound controlled-vehicle movement packet.
#[derive(ReadFrom, Clone, Debug, ServerPacket)]
pub struct SMoveVehicle {
    pub position: DVec3,
    pub y_rot: f32,
    pub x_rot: f32,
    pub on_ground: bool,
}
