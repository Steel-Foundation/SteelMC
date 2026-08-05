use steel_macros::{ReadFrom, ServerPacket};
use steel_utils::types::InteractionHand;

/// Serverbound packet sent when a player uses an item (right-click in air).
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SUseItem {
    pub hand: InteractionHand,

    #[read(as = VarInt)]
    pub sequence: i32,

    pub y_rot: f32,

    pub x_rot: f32,
}
