use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::{
    C_FORGET_LEVEL_CHUNK, C_SET_CHUNK_CACHE_CENTER, C_SET_CHUNK_CACHE_RADIUS,
};
use steel_utils::PackedChunkPos;

#[derive(ClientPacket, WriteTo)]
#[packet_id(Play = C_FORGET_LEVEL_CHUNK)]
pub struct CForgetLevelChunk {
    pub pos: PackedChunkPos,
}

#[derive(ClientPacket, WriteTo)]
#[packet_id(Play = C_SET_CHUNK_CACHE_RADIUS)]
pub struct CSetChunkCacheRadius {
    #[write(as = VarInt)]
    pub radius: i32,
}

#[derive(ClientPacket, WriteTo)]
#[packet_id(Play = C_SET_CHUNK_CACHE_CENTER)]
pub struct CSetChunkCenter {
    #[write(as = VarInt)]
    pub x: i32,
    #[write(as = VarInt)]
    pub y: i32,
}
