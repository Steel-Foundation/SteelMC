use steel_macros::{ReadFrom, ServerPacket};

/// Serverbound packet sent when a player uses the pick item key (middle click) on an entity.
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SPickItemFromEntity {
    #[read(as = VarInt)]
    pub id: i32,
    pub include_data: bool,
}
