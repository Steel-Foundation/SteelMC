use steel_macros::{ClientPacket, WriteTo};

use steel_registry::packets::play::C_FORGET_LEVEL_CHUNK;
use steel_utils::PackedChunkPos;

#[derive(ClientPacket, WriteTo)]
#[packet_id(Play = C_FORGET_LEVEL_CHUNK)]
pub struct CForgetLevelChunk {
    pub pos: PackedChunkPos,
}
