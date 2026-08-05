use steel_macros::{ReadFrom, ServerPacket};
use steel_utils::BlockPos;

/// Serverbound packet sent when a player uses the pick block key (middle click) on a block.
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SPickItemFromBlock {
    pub pos: BlockPos,
    pub include_data: bool,
}
