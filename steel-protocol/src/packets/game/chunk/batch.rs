use steel_macros::{ClientPacket, ReadFrom, ServerPacket, WriteTo};
#[expect(unused_imports)]
use steel_registry::packets::play::S_CHUNK_BATCH_RECEIVED;
use steel_registry::packets::play::{C_CHUNK_BATCH_FINISHED, C_CHUNK_BATCH_START};

#[derive(ClientPacket, WriteTo)]
#[packet_id(Play = C_CHUNK_BATCH_FINISHED)]
pub struct CChunkBatchFinished {
    #[write(as = VarInt)]
    pub batch_size: i32,
}

#[derive(ClientPacket, WriteTo)]
#[packet_id(Play = C_CHUNK_BATCH_START)]
pub struct CChunkBatchStart {}

#[derive(ServerPacket, ReadFrom)]
#[packet_id(Play = S_CHUNK_BATCH_RECEIVED)]
pub struct SChunkBatchReceived {
    pub desired_chunks_per_tick: f32,
}
