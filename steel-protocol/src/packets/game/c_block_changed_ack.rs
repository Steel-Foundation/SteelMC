use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_BLOCK_CHANGED_ACK;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_BLOCK_CHANGED_ACK)]
pub struct CBlockChangedAck {
    #[write(as = VarInt)]
    pub sequence: i32,
}
