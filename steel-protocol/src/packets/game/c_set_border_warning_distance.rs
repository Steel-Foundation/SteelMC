use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_BORDER_WARNING_DISTANCE;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_BORDER_WARNING_DISTANCE)]
pub struct CSetBorderWarningDistance {
    #[write(as = VarInt)]
    pub warning_blocks: i32,
}
