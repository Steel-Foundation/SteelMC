use steel_macros::{ReadFrom, ServerPacket};

#[derive(ServerPacket, ReadFrom, Clone, Debug)]
pub struct SContainerSlotStateChanged {
    #[read(as = VarInt)]
    pub slot_id: i32,
    #[read(as = VarInt)]
    pub container_id: i32,
    pub new_state: bool,
}
