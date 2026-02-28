//! Biome source abstraction for dimension-agnostic biome generation.
//!
//! Mirrors vanilla's `BiomeSource` hierarchy:
//! - `MultiNoiseBiomeSource` — Overworld and Nether (climate parameter matching via `RTree`)
//! - `TheEndBiomeSource` — The End (spatial + erosion threshold)
//!
//! Each dimension creates a different `BiomeSource` implementation. The chunk generator
//! calls `chunk_sampler()` per chunk to get a `ChunkBiomeSampler` that holds per-chunk
//! caches (column cache, `RTree` warm-start index).

use steel_registry::biome::BiomeRef;
use steel_registry::density_functions::OverworldColumnCache;
use steel_registry::density_functions::nether::NetherColumnCache;
use steel_registry::multi_noise::{get_nether_biome_cached, get_overworld_biome_cached};

use super::{NetherClimateSampler, OverworldClimateSampler};

/// Determines biomes at quart positions for a dimension.
///
/// Implementations hold shared state (noise generators, parameter lists) and
/// create per-chunk samplers via [`chunk_sampler`](BiomeSource::chunk_sampler).
pub trait BiomeSource: Send + Sync {
    /// Create a per-chunk biome sampler.
    ///
    /// The returned sampler holds per-chunk caches and should be dropped after
    /// the chunk's biomes are fully populated.
    fn chunk_sampler(&self) -> Box<dyn ChunkBiomeSampler + '_>;
}

/// Per-chunk biome sampler with internal caches.
///
/// Created by [`BiomeSource::chunk_sampler`] for each chunk. Holds caches like
/// column-level density function values and `RTree` warm-start indices that persist
/// across positions within a single chunk.
pub trait ChunkBiomeSampler {
    /// Get the biome at the given quart position.
    fn sample(&mut self, quart_x: i32, quart_y: i32, quart_z: i32) -> BiomeRef;
}

/// Multi-noise biome source for the overworld.
///
/// Uses compiled overworld density functions to sample climate parameters, then
/// looks up the biome in the overworld parameter list (`RTree`).
///
/// Equivalent to vanilla's `MultiNoiseBiomeSource` with the overworld preset.
pub struct OverworldBiomeSource {
    climate_sampler: OverworldClimateSampler,
}

impl OverworldBiomeSource {
    /// Create a new overworld biome source with the given world seed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            climate_sampler: OverworldClimateSampler::new(seed),
        }
    }

    /// Access the underlying climate sampler (for tests, spawn point search, etc.).
    #[must_use]
    pub const fn climate_sampler(&self) -> &OverworldClimateSampler {
        &self.climate_sampler
    }
}

impl BiomeSource for OverworldBiomeSource {
    fn chunk_sampler(&self) -> Box<dyn ChunkBiomeSampler + '_> {
        Box::new(OverworldChunkBiomeSampler {
            source: self,
            column_cache: OverworldColumnCache::new(),
            biome_cache: None,
        })
    }
}

struct OverworldChunkBiomeSampler<'a> {
    source: &'a OverworldBiomeSource,
    column_cache: OverworldColumnCache,
    biome_cache: Option<usize>,
}

impl ChunkBiomeSampler for OverworldChunkBiomeSampler<'_> {
    fn sample(&mut self, quart_x: i32, quart_y: i32, quart_z: i32) -> BiomeRef {
        let target =
            self.source
                .climate_sampler
                .sample(quart_x, quart_y, quart_z, &mut self.column_cache);
        get_overworld_biome_cached(&target, &mut self.biome_cache)
    }
}

// ── Nether ──────────────────────────────────────────────────────────────────

/// Multi-noise biome source for the nether.
///
/// Uses compiled nether density functions to sample temperature and vegetation,
/// then looks up the biome in the nether parameter list (`RTree`).
///
/// Equivalent to vanilla's `MultiNoiseBiomeSource` with the nether preset.
pub struct NetherBiomeSource {
    climate_sampler: NetherClimateSampler,
}

impl NetherBiomeSource {
    /// Create a new nether biome source with the given world seed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            climate_sampler: NetherClimateSampler::new(seed),
        }
    }
}

impl BiomeSource for NetherBiomeSource {
    fn chunk_sampler(&self) -> Box<dyn ChunkBiomeSampler + '_> {
        Box::new(NetherChunkBiomeSampler {
            source: self,
            column_cache: NetherColumnCache::new(),
            biome_cache: None,
        })
    }
}

struct NetherChunkBiomeSampler<'a> {
    source: &'a NetherBiomeSource,
    column_cache: NetherColumnCache,
    biome_cache: Option<usize>,
}

impl ChunkBiomeSampler for NetherChunkBiomeSampler<'_> {
    fn sample(&mut self, quart_x: i32, quart_y: i32, quart_z: i32) -> BiomeRef {
        let target =
            self.source
                .climate_sampler
                .sample(quart_x, quart_y, quart_z, &mut self.column_cache);
        get_nether_biome_cached(&target, &mut self.biome_cache)
    }
}
