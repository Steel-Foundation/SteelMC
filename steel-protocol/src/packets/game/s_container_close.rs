use steel_macros::{ReadFrom, ServerPacket};

#[derive(ServerPacket, ReadFrom, Clone, Debug)]
pub struct SContainerClose {
    #[read(as = VarInt)]
    pub container_id: i32,
}
