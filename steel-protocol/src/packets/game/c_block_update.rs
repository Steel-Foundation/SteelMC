use steel_macros::ClientPacket;
use steel_registry::packets::play::C_BLOCK_UPDATE;
use steel_utils::serial::WriteTo;
use steel_utils::{BlockPos, BlockStateId};

#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_BLOCK_UPDATE)]
pub struct CBlockUpdate {
    pub pos: BlockPos,
    pub block_state: BlockStateId,
}

impl WriteTo for CBlockUpdate {
    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        self.pos.write(writer)?;
        steel_registry::client_content::client_block_state(self.block_state).write(writer)
    }
}
