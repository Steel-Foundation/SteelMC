use steel_macros::ClientPacket;
use steel_registry::packets::play::C_BLOCK_EVENT;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt};
use steel_utils::BlockPos;
use steel_utils::{codec::VarInt, serial::WriteTo};

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
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_BLOCK_EVENT)]
pub struct CBlockEvent {
    /// The position of the block.
    pub pos: BlockPos,
    /// The action ID (block-specific meaning).
    pub action_id: u8,
    /// The action parameter (block-specific meaning).
    pub action_param: u8,
    /// The block registry ID.
    /// Written as `VarInt`.
    pub block_id: i32,
}

impl WriteTo for CBlockEvent {
    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        self.pos.write(writer)?;
        self.action_id.write(writer)?;
        self.action_param.write(writer)?;
        let block_id = if REGISTRY.is_initialized() {
            usize::try_from(self.block_id)
                .ok()
                .and_then(|id| REGISTRY.blocks.by_id(id))
                .map_or(self.block_id, |block| {
                    steel_registry::client_content::client_block(block).id() as i32
                })
        } else {
            self.block_id
        };
        VarInt(block_id).write(writer)
    }
}

impl CBlockEvent {
    /// Creates a new block event packet.
    #[must_use]
    pub const fn new(pos: BlockPos, action_id: u8, action_param: u8, block_id: i32) -> Self {
        Self {
            pos,
            action_id,
            action_param,
            block_id,
        }
    }
}
