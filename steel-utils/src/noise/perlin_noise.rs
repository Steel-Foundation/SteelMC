//! Octave-based Perlin noise implementation matching vanilla Minecraft's `PerlinNoise.java`
//!
//! This combines multiple `ImprovedNoise` instances at different frequencies (octaves)
//! to create more natural-looking noise with detail at multiple scales.

use crate::noise::ImprovedNoise;
use crate::random::{PositionalRandom, Random, RandomSource, RandomSplitter};

/// Round-off constant for coordinate wrapping to prevent precision loss.
/// This is 2^25 = 33554432.
const ROUND_OFF: f64 = 33_554_432.0;

/// Octave-based Perlin noise generator.
///
/// Combines multiple [`ImprovedNoise`] instances at different frequencies
/// to create noise with detail at multiple scales.
#[derive(Debug, Clone)]
pub struct PerlinNoise {
    /// Noise generators for each octave (None if amplitude is 0)
    noise_levels: Vec<Option<ImprovedNoise>>,
    /// Amplitude multipliers for each octave
    amplitudes: Vec<f64>,
    /// Factor applied to input coordinates for the lowest frequency octave
    lowest_freq_input_factor: f64,
    /// Factor applied to output values for the lowest frequency octave
    lowest_freq_value_factor: f64,
    /// Maximum possible output value
    max_value: f64,
}

impl PerlinNoise {
    /// Create a new [`PerlinNoise`] from a positional random splitter (hash-based seeding).
    ///
    /// Each octave gets its seed from `splitter.with_hash_of("octave_{level}")`.
    /// This is a convenience method; for vanilla-matching behavior within [`NormalNoise`],
    /// use [`create_from_random`](Self::create_from_random) instead.
    #[must_use]
    pub fn create(splitter: &RandomSplitter, first_octave: i32, amplitudes: &[f64]) -> Self {
        let octaves = amplitudes.len();
        let zero_octave_index = (-first_octave) as usize;

        let mut noise_levels = vec![None; octaves];

        for i in 0..octaves {
            if amplitudes[i] != 0.0 {
                let octave = first_octave + i as i32;
                let name = format!("octave_{octave}");
                let mut octave_random = splitter.with_hash_of(&name);
                noise_levels[i] = Some(ImprovedNoise::new(&mut octave_random));
            }
        }

        Self::from_parts(noise_levels, amplitudes, zero_octave_index)
    }

    /// Create a new [`PerlinNoise`] from a mutable sequential random source.
    ///
    /// This matches vanilla's [`PerlinNoise`] constructor for [`XoroshiroRandomSource`]:
    /// 1. Consume 262 values from the random (to advance state)
    /// 2. Fork a new positional random from the current state
    /// 3. Use hash-based seeding for each octave from the forked positional
    ///
    /// This is critical for [`NormalNoise`] where the first and second [`PerlinNoise`]
    /// must get different seeds from the same sequential random source.
    #[must_use]
    pub fn create_from_random(
        random: &mut RandomSource,
        first_octave: i32,
        amplitudes: &[f64],
    ) -> Self {
        let octaves = amplitudes.len();
        let zero_octave_index = (-first_octave) as usize;

        // Match vanilla's useNewInitialization=true path:
        // `forkPositional()` consumes 2 longs from the random source
        let splitter = random.next_positional();

        let mut noise_levels = vec![None; octaves];

        for i in 0..octaves {
            if amplitudes[i] != 0.0 {
                let octave = first_octave + i as i32;
                let name = format!("octave_{octave}");
                let mut octave_random = splitter.with_hash_of(&name);
                noise_levels[i] = Some(ImprovedNoise::new(&mut octave_random));
            }
        }

        Self::from_parts(noise_levels, amplitudes, zero_octave_index)
    }

    /// Create a [`PerlinNoise`] using the legacy nether biome initialization path.
    ///
    /// Unlike [`create_from_random`](Self::create_from_random) which uses positional/hash-based
    /// seeding, this creates `ImprovedNoise` instances directly from a sequential random source.
    /// Matches vanilla's `PerlinNoise(random, pair, useNewInitialization=false)`.
    #[must_use]
    pub fn create_legacy_for_nether(
        random: &mut RandomSource,
        first_octave: i32,
        amplitudes: &[f64],
    ) -> Self {
        let octaves = amplitudes.len();
        let zero_octave_index = (-first_octave) as usize;

        let mut noise_levels = vec![None; octaves];

        // Create the zero-octave noise level first (directly from random)
        let zero_octave = ImprovedNoise::new(random);
        if zero_octave_index < octaves && amplitudes[zero_octave_index] != 0.0 {
            noise_levels[zero_octave_index] = Some(zero_octave);
        }

        // Walk backwards from zero-octave, creating or skipping octaves
        for ix in (0..zero_octave_index).rev() {
            if ix < octaves {
                if amplitudes[ix] == 0.0 {
                    // Skip: consume 262 values to advance random state.
                    // 262 = ImprovedNoise::new() consumption: 3 nextDouble() calls (offsets)
                    // + 256 nextInt() calls (Fisher-Yates shuffle) + 3 loop iterations that
                    // call nextInt() for the final swaps = 262 total random advances.
                    random.consume_count(262);
                } else {
                    noise_levels[ix] = Some(ImprovedNoise::new(random));
                }
            } else {
                random.consume_count(262);
            }
        }

        Self::from_parts(noise_levels, amplitudes, zero_octave_index)
    }

    /// Build a [`PerlinNoise`] from pre-computed noise levels.
    #[must_use]
    fn from_parts(
        noise_levels: Vec<Option<ImprovedNoise>>,
        amplitudes: &[f64],
        zero_octave_index: usize,
    ) -> Self {
        let octaves = amplitudes.len();

        // Calculate frequency factors
        // lowest_freq_input_factor = 2^(-zero_octave_index)
        let lowest_freq_input_factor = 2.0_f64.powi(-(zero_octave_index as i32));

        // lowest_freq_value_factor = 2^(octaves-1) / (2^octaves - 1)
        let lowest_freq_value_factor =
            2.0_f64.powi((octaves - 1) as i32) / (2.0_f64.powi(octaves as i32) - 1.0);

        // Calculate max value
        let max_value = Self::edge_value(amplitudes, lowest_freq_value_factor, 2.0);

        Self {
            noise_levels,
            amplitudes: amplitudes.to_vec(),
            lowest_freq_input_factor,
            lowest_freq_value_factor,
            max_value,
        }
    }

    /// Calculate the theoretical maximum value for the given amplitudes.
    fn edge_value(amplitudes: &[f64], lowest_freq_value_factor: f64, noise_value: f64) -> f64 {
        let mut value = 0.0;
        let mut value_factor = lowest_freq_value_factor;

        for &amplitude in amplitudes {
            if amplitude != 0.0 {
                value += amplitude * noise_value * value_factor;
            }
            value_factor /= 2.0;
        }

        value
    }

    /// Sample the noise at the given coordinates.
    #[inline]
    #[must_use]
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        self.get_value_with_y_params(x, y, z, 0.0, 0.0, false)
    }

    /// Sample the noise with Y scaling parameters.
    ///
    /// # Arguments
    /// * `x`, `y`, `z` - Coordinates to sample
    /// * `y_scale` - Y scaling factor for terrain
    /// * `y_fudge` - Y fudge factor for floor snapping
    /// * `y_flat_hack` - If true, use `-yo` instead of wrapped y (for legacy biomes)
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn get_value_with_y_params(
        &self,
        x: f64,
        y: f64,
        z: f64,
        y_scale: f64,
        y_fudge: f64,
        y_flat_hack: bool,
    ) -> f64 {
        let mut value = 0.0;
        let mut input_factor = self.lowest_freq_input_factor;
        let mut value_factor = self.lowest_freq_value_factor;

        for (i, noise_opt) in self.noise_levels.iter().enumerate() {
            if let Some(noise) = noise_opt {
                let noise_val = noise.noise_with_y_scale(
                    wrap(x * input_factor),
                    if y_flat_hack {
                        -noise.yo
                    } else {
                        wrap(y * input_factor)
                    },
                    wrap(z * input_factor),
                    y_scale * input_factor,
                    y_fudge * input_factor,
                );
                value += self.amplitudes[i] * noise_val * value_factor;
            }

            input_factor *= 2.0;
            value_factor /= 2.0;
        }

        value
    }

    /// Get the maximum possible output value.
    #[inline]
    #[must_use]
    pub const fn max_value(&self) -> f64 {
        self.max_value
    }

    /// Get the noise generator for a specific octave (by index from highest frequency).
    ///
    /// Index 0 is the highest frequency octave.
    #[must_use]
    pub fn get_octave_noise(&self, i: usize) -> Option<&ImprovedNoise> {
        self.noise_levels
            .get(self.noise_levels.len() - 1 - i)
            .and_then(|opt| opt.as_ref())
    }
}

/// Wrap a coordinate to prevent precision loss at large values.
///
/// This wraps the coordinate to the range `[-ROUND_OFF/2, ROUND_OFF/2]` to
/// maintain numerical precision for coordinates far from the origin.
#[inline]
#[must_use]
fn wrap(x: f64) -> f64 {
    x - (x / ROUND_OFF + 0.5).floor() * ROUND_OFF
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::{Random, xoroshiro::Xoroshiro};

    #[test]
    fn test_perlin_noise_deterministic() {
        let mut rng = Xoroshiro::from_seed(12345);
        let splitter = rng.next_positional();

        let amplitudes = [1.0, 1.0, 1.0];
        let noise1 = PerlinNoise::create(&splitter, -3, &amplitudes);
        let noise2 = PerlinNoise::create(&splitter, -3, &amplitudes);

        let v1 = noise1.get_value(100.0, 64.0, 100.0);
        let v2 = noise2.get_value(100.0, 64.0, 100.0);
        assert!((v1 - v2).abs() < 1e-15);
    }

    #[test]
    fn test_perlin_noise_spatial_variation() {
        let mut rng = Xoroshiro::from_seed(42);
        let splitter = rng.next_positional();

        let noise = PerlinNoise::create(&splitter, -4, &[1.0, 1.0, 1.0, 1.0]);

        // Sample at different locations
        let values: Vec<f64> = (0..10)
            .map(|i| noise.get_value(f64::from(i) * 50.0, 64.0, f64::from(i) * 50.0))
            .collect();

        // Check there's variation
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(max - min > 0.01, "Noise should have spatial variation");
    }

    #[test]
    fn test_create_from_random_different_seeds() {
        let mut rng = Xoroshiro::from_seed(12345);
        let splitter = rng.next_positional();
        let mut random = splitter.with_hash_of("test_noise");

        let amplitudes = [1.0, 1.0, 1.0];
        let noise1 = PerlinNoise::create_from_random(&mut random, -3, &amplitudes);
        let noise2 = PerlinNoise::create_from_random(&mut random, -3, &amplitudes);

        // These should produce different values since the random state advanced
        let v1 = noise1.get_value(100.0, 64.0, 100.0);
        let v2 = noise2.get_value(100.0, 64.0, 100.0);
        assert!(
            (v1 - v2).abs() > 0.001,
            "Two PerlinNoise from sequential random should differ: v1={v1}, v2={v2}",
        );
    }

    #[test]
    fn test_wrap() {
        // Small values should be unchanged
        assert!((wrap(100.0) - 100.0).abs() < 1e-10);
        assert!((wrap(-100.0) - (-100.0)).abs() < 1e-10);

        // Very large values should be wrapped
        let large = 100_000_000.0;
        let wrapped = wrap(large);
        assert!(wrapped.abs() < ROUND_OFF);
    }
}
