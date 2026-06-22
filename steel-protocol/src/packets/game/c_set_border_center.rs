use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_BORDER_CENTER;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_BORDER_CENTER)]
pub struct CSetBorderCenter {
    pub new_center_x: f64,
    pub new_center_z: f64,
}
