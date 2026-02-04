use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_LEVEL_CHUNK_WITH_LIGHT;
use steel_utils::serial::OptionalNbt;
use steel_utils::{ChunkPos, codec::BitSet};

#[derive(WriteTo, Copy, Clone, Debug)]
#[write(as = VarInt)]
pub enum HeightmapType {
    WorldSurface = 1,
    MotionBlocking = 4,
    MotionBlockingNoLeaves = 5,
}

#[derive(Debug, Clone, WriteTo)]
pub struct Heightmaps {
    pub heightmaps: Vec<(HeightmapType, Vec<i64>)>,
}

#[derive(Debug, Clone, WriteTo)]
pub struct BlockEntityInfo {
    pub packed_xz: u8,
    pub y: i16,
    #[write(as = VarInt)]
    pub type_id: i32,
    pub data: OptionalNbt,
}

#[derive(Debug, Clone, WriteTo)]
pub struct ChunkPacketData {
    pub heightmaps: Heightmaps,
    pub data: Vec<u8>,
    pub block_entities: Vec<BlockEntityInfo>,
}

#[derive(Debug, Clone, WriteTo)]
pub struct LightUpdatePacketData {
    pub sky_y_mask: BitSet,
    pub block_y_mask: BitSet,
    pub empty_sky_y_mask: BitSet,
    pub empty_block_y_mask: BitSet,
    pub sky_updates: Vec<Vec<u8>>,
    pub block_updates: Vec<Vec<u8>>,
}

#[derive(ClientPacket, Debug, Clone, WriteTo)]
#[packet_id(Play = C_LEVEL_CHUNK_WITH_LIGHT)]
pub struct CLevelChunkWithLight {
    pub pos: ChunkPos,
    pub chunk_data: ChunkPacketData,
    pub light_data: LightUpdatePacketData,
}
