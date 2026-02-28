//! Climate sampler for nether world generation.
//!
//! Uses the compiled nether density functions from steel-registry. The nether's
//! noise router is much simpler than the overworld — only temperature and vegetation
//! are real density functions; all other climate parameters are constant 0.
//!
//! The nether uses `legacy_random_source=true` which means noise generators are
//! created with Java's `java.util.Random` (LCG) instead of Xoroshiro, and
//! temperature/vegetation use hardcoded parameters `(-7, [1.0, 1.0])` via
//! `NormalNoise.createLegacyNetherBiome()`. The shift (offset) noise is effectively
//! zeroed with params `(0, [0.0])`. See `RandomState.java:55-76`.

use steel_registry::density_functions::nether::{self, NetherColumnCache, NetherNoises};
use steel_utils::climate::{TargetPoint, quantize_coord};
use steel_utils::noise::NormalNoise;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::{PositionalRandom, Random, RandomSource};

/// Climate sampler for the nether using compiled density functions.
///
/// Only evaluates temperature and vegetation from the nether noise router.
/// Continentalness, erosion, depth, and weirdness are always 0 in the nether.
pub struct NetherClimateSampler {
    /// Noise generators needed by the nether density functions.
    noises: Box<NetherNoises>,
}

impl NetherClimateSampler {
    /// Create a new nether climate sampler with the given seed.
    ///
    /// Uses the legacy random source path matching vanilla's `RandomState` with
    /// `useLegacyRandomSource=true`:
    /// - Temperature: `LegacyRandomSource(seed + 0)`, `createLegacyNetherBiome`, params `(-7, [1.0, 1.0])`
    /// - Vegetation: `LegacyRandomSource(seed + 1)`, `createLegacyNetherBiome`, params `(-7, [1.0, 1.0])`
    /// - Offset (shift): `random.fromHashOf("minecraft:offset")`, regular create, params `(0, [0.0])`
    #[must_use]
    pub fn new(seed: u64) -> Self {
        // Temperature: LegacyRandomSource(seed + 0), legacy nether biome path
        let mut temp_rng = RandomSource::Legacy(LegacyRandom::from_seed(seed));
        let n_temperature = NormalNoise::create_legacy_nether_biome(&mut temp_rng, -7, &[1.0, 1.0]);

        // Vegetation: LegacyRandomSource(seed + 1), legacy nether biome path
        let mut veg_rng = RandomSource::Legacy(LegacyRandom::from_seed(seed.wrapping_add(1)));
        let n_vegetation = NormalNoise::create_legacy_nether_biome(&mut veg_rng, -7, &[1.0, 1.0]);

        // Offset (shift) noise: LegacyRandom(seed).forkPositional().fromHashOf("minecraft:offset")
        // With params (0, [0.0]) — effectively zero, making nether shifts negligible.
        let mut rng = LegacyRandom::from_seed(seed);
        let splitter = rng.next_positional();
        let mut offset_rng = splitter.with_hash_of("minecraft:offset");
        let n_offset = NormalNoise::create_from_random(&mut offset_rng, 0, &[0.0]);

        let noises = NetherNoises {
            n_offset,
            n_temperature,
            n_vegetation,
        };

        Self {
            noises: Box::new(noises),
        }
    }

    /// Sample climate at a quart position.
    ///
    /// The `cache` holds column-level (xz-only) precomputed values.
    #[must_use]
    pub fn sample(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        cache: &mut NetherColumnCache,
    ) -> TargetPoint {
        let block_x = quart_x << 2;
        let block_y = quart_y << 2;
        let block_z = quart_z << 2;

        cache.ensure(block_x, block_z, &self.noises);

        let temp =
            nether::router_temperature(&self.noises, cache, block_x, block_y, block_z) as f32;
        let humidity =
            nether::router_vegetation(&self.noises, cache, block_x, block_y, block_z) as f32;

        // Nether noise router has continentalness, erosion, depth, ridges all as constant 0.
        TargetPoint::new(
            quantize_coord(f64::from(temp)),
            quantize_coord(f64::from(humidity)),
            0,
            0,
            0,
            0,
        )
    }
}
