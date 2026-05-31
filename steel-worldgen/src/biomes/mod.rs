mod biome_source;
mod climate_sampler;
mod nether_climate_sampler;

use sha2::{Digest, Sha256};

pub use biome_source::{
    BiomeSourceKind, ChunkBiomeSampler, EndBiomeSource, NetherBiomeSource, OverworldBiomeSource,
};
pub use climate_sampler::OverworldClimateSampler;
pub use nether_climate_sampler::NetherClimateSampler;

/// Matches vanilla `BiomeManager.obfuscateSeed(long)`.
#[must_use]
pub fn obfuscate_biome_seed(seed: i64) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(seed.to_le_bytes());
    let result = hasher.finalize();
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&result[..8]);
    i64::from_le_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::obfuscate_biome_seed;

    #[test]
    fn obfuscate_biome_seed_matches_guava_hash_long() {
        assert_eq!(obfuscate_biome_seed(0), 8_794_265_229_978_523_055);
        assert_eq!(obfuscate_biome_seed(1), -6_467_378_160_175_308_932);
        assert_eq!(obfuscate_biome_seed(12_345), 293_737_985_876_514_017);
        assert_eq!(obfuscate_biome_seed(-1), 6_759_447_113_877_070_610);
    }
}
