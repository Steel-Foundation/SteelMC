use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_CHUNK_CACHE_RADIUS;

#[derive(ClientPacket, WriteTo)]
#[packet_id(Play = C_SET_CHUNK_CACHE_RADIUS)]
pub struct CSetChunkCacheRadius {
    #[write(as = VarInt)]
    pub radius: i32,
}
