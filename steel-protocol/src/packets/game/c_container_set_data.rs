use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_CONTAINER_SET_DATA;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_CONTAINER_SET_DATA)]
pub struct CContainerSetData {
    #[write(as = VarInt)]
    pub container_id: i32,
    pub id: i16,
    pub value: i16,
}
