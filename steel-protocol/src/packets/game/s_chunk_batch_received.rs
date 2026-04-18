use steel_macros::{ReadFrom, ServerPacket};
#[expect(unused_imports)]
use steel_registry::packets::play::S_CHUNK_BATCH_RECEIVED;

#[derive(ServerPacket, ReadFrom)]
#[packet_id(Play = S_CHUNK_BATCH_RECEIVED)]
pub struct SChunkBatchReceived {
    pub desired_chunks_per_tick: f32,
}
