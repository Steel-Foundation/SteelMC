//! Biome source abstraction for dimension-agnostic biome generation.
//!
//! Mirrors vanilla's `BiomeSource` hierarchy:
//! - `MultiNoiseBiomeSource` — Overworld and Nether (climate parameter matching via `RTree`)
//! - `TheEndBiomeSource` — The End (spatial + erosion threshold)
//!
//! Each dimension creates a different `BiomeSourceKind` variant. The chunk generator
//! calls `chunk_sampler()` per chunk to get a `ChunkBiomeSampler` that holds per-chunk
//! caches (column cache, `RTree` warm-start index).

use steel_registry::biome::BiomeRef;
use steel_registry::density_functions::OverworldColumnCache;
use steel_registry::density_functions::nether::NetherColumnCache;
use steel_registry::multi_noise::{get_nether_biome_cached, get_overworld_biome_cached};
use steel_registry::vanilla_biomes;

use super::end_islands::EndIslands;
use super::{NetherClimateSampler, OverworldClimateSampler};

/// Dimension-specific biome source.
///
/// Each variant holds shared state (noise generators, parameter lists) and
/// creates per-chunk samplers via [`chunk_sampler`](BiomeSourceKind::chunk_sampler).
#[non_exhaustive]
pub enum BiomeSourceKind {
    /// Overworld biome source (multi-noise climate matching).
    Overworld(OverworldBiomeSource),
    /// Nether biome source (multi-noise temperature/vegetation matching).
    Nether(NetherBiomeSource),
    /// End biome source (spatial distance + erosion threshold).
    ///
    /// Boxed because `EndIslands` is ~2KB (simplex noise permutation table),
    /// while the other variants are pointer-sized.
    End(Box<EndBiomeSource>),
}

impl BiomeSourceKind {
    /// Create an overworld biome source with the given world seed.
    #[must_use]
    pub fn overworld(seed: u64) -> Self {
        Self::Overworld(OverworldBiomeSource::new(seed))
    }

    /// Create a nether biome source with the given world seed.
    #[must_use]
    pub fn nether(seed: u64) -> Self {
        Self::Nether(NetherBiomeSource::new(seed))
    }

    /// Create an end biome source with the given world seed.
    #[must_use]
    pub fn end(seed: u64) -> Self {
        Self::End(Box::new(EndBiomeSource::new(seed)))
    }

    /// Create a per-chunk biome sampler.
    ///
    /// The returned sampler holds per-chunk caches and should be dropped after
    /// the chunk's biomes are fully populated.
    #[must_use]
    pub fn chunk_sampler(&self) -> ChunkBiomeSampler<'_> {
        match self {
            Self::Overworld(source) => source.chunk_sampler(),
            Self::Nether(source) => source.chunk_sampler(),
            Self::End(source) => source.chunk_sampler(),
        }
    }
}

/// Per-chunk biome sampler with internal caches.
///
/// Created by [`BiomeSourceKind::chunk_sampler`] for each chunk. Holds caches like
/// column-level density function values and `RTree` warm-start indices that persist
/// across positions within a single chunk.
///
/// Uses enum dispatch instead of `dyn` to avoid vtable overhead on the hot
/// per-quart sampling path (1536 calls per overworld chunk).
pub enum ChunkBiomeSampler<'a> {
    /// Overworld sampler (climate → R-tree lookup).
    Overworld(OverworldChunkBiomeSampler<'a>),
    /// Nether sampler (climate → R-tree lookup).
    Nether(NetherChunkBiomeSampler<'a>),
    /// End sampler (spatial distance thresholds).
    End(EndChunkBiomeSampler<'a>),
}

impl ChunkBiomeSampler<'_> {
    /// Get the biome at the given quart position.
    #[inline]
    pub fn sample(&mut self, quart_x: i32, quart_y: i32, quart_z: i32) -> BiomeRef {
        match self {
            Self::Overworld(s) => s.sample(quart_x, quart_y, quart_z),
            Self::Nether(s) => s.sample(quart_x, quart_y, quart_z),
            Self::End(s) => s.sample(quart_x, quart_y, quart_z),
        }
    }
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

    fn chunk_sampler(&self) -> ChunkBiomeSampler<'_> {
        ChunkBiomeSampler::Overworld(OverworldChunkBiomeSampler {
            source: self,
            column_cache: OverworldColumnCache::new(),
            biome_cache: None,
        })
    }
}

pub struct OverworldChunkBiomeSampler<'a> {
    source: &'a OverworldBiomeSource,
    column_cache: OverworldColumnCache,
    biome_cache: Option<usize>,
}

impl OverworldChunkBiomeSampler<'_> {
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

    fn chunk_sampler(&self) -> ChunkBiomeSampler<'_> {
        ChunkBiomeSampler::Nether(NetherChunkBiomeSampler {
            source: self,
            column_cache: NetherColumnCache::new(),
            biome_cache: None,
        })
    }
}

pub struct NetherChunkBiomeSampler<'a> {
    source: &'a NetherBiomeSource,
    column_cache: NetherColumnCache,
    biome_cache: Option<usize>,
}

impl NetherChunkBiomeSampler<'_> {
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

    const fn chunk_sampler(&self) -> ChunkBiomeSampler<'_> {
        ChunkBiomeSampler::End(EndChunkBiomeSampler { source: self })
    }
}

pub struct EndChunkBiomeSampler<'a> {
    source: &'a EndBiomeSource,
}

impl EndChunkBiomeSampler<'_> {
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
