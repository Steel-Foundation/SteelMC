use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_BORDER_WARNING_DELAY;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_BORDER_WARNING_DELAY)]
pub struct CSetBorderWarningDelay {
    #[write(as = VarInt)]
    pub warning_delay: i32,
}
