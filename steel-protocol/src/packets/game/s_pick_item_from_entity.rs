use steel_macros::{ReadFrom, ServerPacket};

/// Serverbound packet sent when a player uses the pick block key (middle click) on a entity.
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SPickItemFromEntity {
    #[read(as = VarInt)]
    pub entity_id: i32,
    pub include_data: bool,
}
