use steel_macros::{ReadFrom, ServerPacket};

/// Serverbound packet sent when a player left-click attacks an entity.
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SAttack {
    #[read(as = VarInt)]
    pub entity_id: i32,
}
