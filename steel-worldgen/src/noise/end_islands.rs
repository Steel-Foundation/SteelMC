//! End islands terrain generation algorithm.
//!
//! Matches vanilla's `EndIslandFunction`, the simplex-noise-driven outer-island
//! part of The End density graph.
//!
//! The simplex permutation uses the world seed after `consumeCount(17292)`.
//!
//! Result range: `[-0.84375, 0.5625]`.

use crate::random::Random;
use crate::random::legacy_random::LegacyRandom;

use super::SimplexNoise;

/// Threshold for simplex noise below which an island is spawned.
///
/// Stored widened because [`SimplexNoise`] exposes its Vanilla-f32 result as f64.
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
    /// Matches `EndIslandFunction.compileSampler` and `RandomState`'s compile context.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut rng = LegacyRandom::from_seed(seed);
        rng.consume_count(17292);
        let island_noise = SimplexNoise::new_without_noise_offset(&mut rng);
        Self { island_noise }
    }

    /// Sample the density value at block coordinates.
    ///
    /// Converts block coordinates to section coordinates internally (divides by 8).
    #[must_use]
    pub fn sample(&self, block_x: f64, _block_y: f64, block_z: f64) -> f64 {
        let block_x = block_x as i32;
        let block_z = block_z as i32;
        f64::from(
            (Self::get_height_value(&self.island_noise, block_x / 8, block_z / 8) - 8.0_f32)
                / 128.0_f32,
        )
    }

    /// Compute the height value at section coordinates.
    ///
    /// Matches vanilla's `EndIslandFunction.getHeightValue()`.
    /// Takes section coordinates (block position / 8).
    fn get_height_value(island_noise: &SimplexNoise, section_x: i32, section_z: i32) -> f32 {
        let chunk_x = section_x / 2;
        let chunk_z = section_z / 2;
        let sub_section_x = section_x % 2;
        let sub_section_z = section_z % 2;

        let mut doffs = -100.0_f32;

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
                    if new_doffs > doffs {
                        doffs = new_doffs;
                    }
                }
            }
        }

        doffs
    }
}
