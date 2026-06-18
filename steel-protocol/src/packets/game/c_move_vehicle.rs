use glam::DVec3;
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_MOVE_VEHICLE;

/// Clientbound controlled-vehicle position correction packet.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_MOVE_VEHICLE)]
pub struct CMoveVehicle {
    pub position: DVec3,
    pub y_rot: f32,
    pub x_rot: f32,
}
