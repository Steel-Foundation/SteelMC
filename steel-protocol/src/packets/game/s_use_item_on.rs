use steel_macros::{ReadFrom, ServerPacket};
use steel_registry::items::item::BlockHitResult;
use steel_utils::types::InteractionHand;

/// Serverbound packet sent when a player uses an item on a block (right-click on block).
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SUseItemOn {
    pub hand: InteractionHand,

    pub block_hit: BlockHitResult,

    #[read(as = VarInt)]
    pub sequence: i32,
}
