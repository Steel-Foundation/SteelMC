use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_CHUNKS_BIOMES;
use steel_utils::PackedChunkPos;

/// One chunk's biome containers, one per section from the bottom up.
#[derive(Debug, Clone, WriteTo)]
pub struct ChunkBiomeData {
    pub pos: PackedChunkPos,
    #[write(as = Prefixed(VarInt), bound = 2097152)]
    pub buffer: Vec<u8>,
}

/// Replaces the biomes of loaded chunks without resending their blocks.
///
/// Vanilla's `ClientboundChunksBiomesPacket`, sent by `/fillbiome`.
#[derive(ClientPacket, Debug, Clone, WriteTo)]
#[packet_id(Play = C_CHUNKS_BIOMES)]
pub struct CChunksBiomes {
    pub chunks: Vec<ChunkBiomeData>,
}

#[cfg(test)]
mod tests {
    use steel_utils::serial::WriteTo as _;
    use steel_utils::{ChunkPos, PackedChunkPos};

    use super::*;

    #[test]
    fn chunks_biomes_matches_vanilla_layout() -> std::io::Result<()> {
        let pos = ChunkPos::new(3, -2);
        let packet = CChunksBiomes {
            chunks: vec![ChunkBiomeData {
                pos: PackedChunkPos::from(pos),
                buffer: vec![0, 7],
            }],
        };
        let mut bytes = Vec::new();
        packet.write(&mut bytes)?;

        let mut expected = vec![1];
        expected.extend_from_slice(&PackedChunkPos::from(pos).as_raw().to_be_bytes());
        expected.extend_from_slice(&[2, 0, 7]);
        assert_eq!(bytes, expected);
        Ok(())
    }
}
