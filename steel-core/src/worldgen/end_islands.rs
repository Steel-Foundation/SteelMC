//! End islands terrain generation algorithm.
//!
//! Matches vanilla's `DensityFunctions.EndIslandDensityFunction`. Generates the
//! characteristic floating island pattern of The End by combining a distance-based
//! falloff from the origin with simplex-noise-driven island placement.
//!
//! The noise seed is always 0 (world-seed-independent), initialized with
//! `LegacyRandomSource(0)` + `consumeCount(17292)`.
//!
//! Result range: `[-0.84375, 0.5625]`.

use steel_utils::noise::SimplexNoise;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;

/// Threshold for simplex noise below which an island is spawned.
///
/// Vanilla uses `-0.9F` (float literal) in a `double < float` comparison, which
/// promotes the float to double. `(double)(-0.9f)` ≈ `-0.8999999761581421`,
/// NOT the exact double `-0.9`. We must match this f32→f64 promotion.
const ISLAND_THRESHOLD: f64 = -0.9_f32 as f64;

/// End islands density function.
///
/// Unlike overworld/nether density functions which are transpiled into native Rust,
/// this is used directly at runtime because it's a self-contained leaf algorithm
/// (simplex noise + neighbor loop) with no density function tree to transpile.
#[derive(Debug, Clone)]
pub struct EndIslands {
    island_noise: SimplexNoise,
}

impl EndIslands {
    /// Create a new `EndIslands` with the given world seed.
    ///
    /// Matches vanilla's `RandomState.NoiseWiringHelper.wrapNew()` which creates
    /// `EndIslandDensityFunction(worldSeed)`, NOT seed 0. The JSON codec defaults
    /// to seed 0, but `RandomState` replaces it with the world seed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut rng = LegacyRandom::from_seed(seed);
        rng.consume_count(17292);
        let island_noise = SimplexNoise::new(&mut rng);
        Self { island_noise }
    }

    /// Sample the erosion value at block coordinates.
    ///
    /// Used by `EndBiomeSource` to determine biome thresholds. Converts block
    /// coordinates to section coordinates internally (divides by 8).
    #[must_use]
    pub fn sample(&self, block_x: i32, _block_y: i32, block_z: i32) -> f64 {
        // Widen to f64 BEFORE subtracting 8.0, matching Java's `float - 8.0` (double literal)
        // where the float is promoted to double first.
        (f64::from(Self::get_height_value(
            &self.island_noise,
            block_x / 8,
            block_z / 8,
        )) - 8.0)
            / 128.0
    }

    /// Compute the height value at section coordinates.
    ///
    /// Matches vanilla's `EndIslandDensityFunction.getHeightValue()`.
    /// Takes section coordinates (block position / 8).
    fn get_height_value(island_noise: &SimplexNoise, section_x: i32, section_z: i32) -> f32 {
        let chunk_x = section_x / 2;
        let chunk_z = section_z / 2;
        let sub_section_x = section_x % 2;
        let sub_section_z = section_z % 2;

        // Distance-based falloff from the origin.
        // Vanilla does integer multiply THEN casts to float: `Mth.sqrt(sectionX * sectionX + ...)`.
        // Integer overflow wraps in Java; we use wrapping_mul/wrapping_add to match.
        let dist_sq = section_x
            .wrapping_mul(section_x)
            .wrapping_add(section_z.wrapping_mul(section_z));
        let dist = (dist_sq as f32).sqrt();
        let mut doffs = (100.0_f32 - dist * 8.0).clamp(-100.0, 80.0);

        // Check 25×25 neighborhood for island contributions
        for xo in -12..=12 {
            for zo in -12..=12 {
                let total_chunk_x = i64::from(chunk_x) + i64::from(xo);
                let total_chunk_z = i64::from(chunk_z) + i64::from(zo);

                if total_chunk_x * total_chunk_x + total_chunk_z * total_chunk_z > 4096
                    && island_noise.get_value_2d(total_chunk_x as f64, total_chunk_z as f64)
                        < ISLAND_THRESHOLD
                {
                    let island_size = ((total_chunk_x as f32).abs() * 3439.0
                        + (total_chunk_z as f32).abs() * 147.0)
                        % 13.0
                        + 9.0;
                    let xd = sub_section_x as f32 - (xo * 2) as f32;
                    let zd = sub_section_z as f32 - (zo * 2) as f32;
                    let new_doffs =
                        (100.0_f32 - (xd * xd + zd * zd).sqrt() * island_size).clamp(-100.0, 80.0);
                    doffs = doffs.max(new_doffs);
                }
            }
        }

        doffs
    }
}
