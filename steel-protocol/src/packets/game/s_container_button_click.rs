use steel_macros::{ReadFrom, ServerPacket};

#[derive(ServerPacket, ReadFrom, Clone, Debug)]
pub struct SContainerButtonClick {
    #[read(as = VarInt)]
    pub container_id: i32,
    #[read(as = VarInt)]
    pub button_id: i32,
}
