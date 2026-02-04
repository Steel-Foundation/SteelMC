use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_BLOCK_EVENT;
use steel_utils::BlockPos;

/// Sent to trigger block-specific events on the client.
///
/// Block events are used for special block behaviors like:
/// - Pistons extending/retracting
/// - Note blocks playing notes
/// - Chest lid opening/closing
/// - Ender chests
/// - Bells ringing
///
/// Each block type interprets the `action_id` and `action_param` differently.
#[derive(WriteTo, ClientPacket, Clone, Debug)]
#[packet_id(Play = C_BLOCK_EVENT)]
pub struct CBlockEvent {
    /// The position of the block.
    pub pos: BlockPos,
    /// The action ID (block-specific meaning).
    pub action_id: u8,
    /// The action parameter (block-specific meaning).
    pub action_param: u8,
    /// The block registry ID.
    /// Written as VarInt.
    #[write(as = VarInt)]
    pub block_id: i32,
}

impl CBlockEvent {
    /// Creates a new block event packet.
    #[must_use]
    pub fn new(pos: BlockPos, action_id: u8, action_param: u8, block_id: i32) -> Self {
        Self {
            pos,
            action_id,
            action_param,
            block_id,
        }
    }
}
