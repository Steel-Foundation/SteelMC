use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_BORDER_LERP_SIZE;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_BORDER_LERP_SIZE)]
pub struct CSetBorderLerpSize {
    pub old_size: f64,
    pub new_size: f64,
    #[write(as = VarLong)]
    pub lerp_time: i64,
}
