use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_CONTAINER_CLOSE;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_CONTAINER_CLOSE)]
pub struct CContainerClose {
    #[write(as = VarInt)]
    pub container_id: i32,
}
