use steel_macros::{ReadFrom, ServerPacket};

#[derive(ServerPacket, ReadFrom, Clone, Debug)]
pub struct SSelectTrade {
    #[read(as = VarInt)]
    pub item: i32,
}
