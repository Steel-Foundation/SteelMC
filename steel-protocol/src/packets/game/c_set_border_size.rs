use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_BORDER_SIZE;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_BORDER_SIZE)]
pub struct CSetBorderSize {
    pub size: f64,
}
