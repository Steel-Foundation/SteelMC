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
use steel_registry::vanilla_biomes;

use super::end_islands::EndIslands;
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

// ── The End ───────────────────────────────────────────────────────────────────

/// Biome source for The End dimension.
///
/// Uses spatial distance from origin and the `EndIslands` density function for
/// biome selection. Does NOT use climate parameters — biome choice is based on:
///
/// 1. **Central island** (`chunkX² + chunkZ² ≤ 4096`): always `the_end`
/// 2. **Outer islands** (erosion from `EndIslands` at transformed coordinates):
///    - `> 0.25` → `end_highlands`
///    - `≥ -0.0625` → `end_midlands`
///    - `< -0.21875` → `small_end_islands`
///    - otherwise → `end_barrens`
///
/// Matches vanilla's `TheEndBiomeSource`.
pub struct EndBiomeSource {
    end_islands: EndIslands,
}

impl EndBiomeSource {
    /// Create a new End biome source with the given world seed.
    ///
    /// The `EndIslands` density function is initialized with the world seed,
    /// matching vanilla's `RandomState.NoiseWiringHelper.wrapNew()` which replaces
    /// the default seed-0 instance with `EndIslandDensityFunction(worldSeed)`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            end_islands: EndIslands::new(seed),
        }
    }
}

impl BiomeSource for EndBiomeSource {
    fn chunk_sampler(&self) -> Box<dyn ChunkBiomeSampler + '_> {
        Box::new(EndChunkBiomeSampler { source: self })
    }
}

struct EndChunkBiomeSampler<'a> {
    source: &'a EndBiomeSource,
}

impl ChunkBiomeSampler for EndChunkBiomeSampler<'_> {
    fn sample(&mut self, quart_x: i32, quart_y: i32, quart_z: i32) -> BiomeRef {
        let block_x = quart_x << 2;
        let block_y = quart_y << 2;
        let block_z = quart_z << 2;
        let chunk_x = block_x >> 4;
        let chunk_z = block_z >> 4;

        // Central island: if within 64 chunks of origin
        if i64::from(chunk_x) * i64::from(chunk_x) + i64::from(chunk_z) * i64::from(chunk_z) <= 4096
        {
            return &vanilla_biomes::THE_END;
        }

        // Outer islands: sample erosion (EndIslands) at transformed coordinates
        let weird_block_x = (chunk_x * 2 + 1) * 8;
        let weird_block_z = (chunk_z * 2 + 1) * 8;
        let erosion = self
            .source
            .end_islands
            .sample(weird_block_x, block_y, weird_block_z);

        if erosion > 0.25 {
            &vanilla_biomes::END_HIGHLANDS
        } else if erosion >= -0.0625 {
            &vanilla_biomes::END_MIDLANDS
        } else if erosion < -0.21875 {
            &vanilla_biomes::SMALL_END_ISLANDS
        } else {
            &vanilla_biomes::END_BARRENS
        }
    }
}
