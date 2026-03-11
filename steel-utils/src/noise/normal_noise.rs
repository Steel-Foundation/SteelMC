//! Normal (Double Perlin) noise implementation matching vanilla Minecraft's NormalNoise.java
//!
//! This combines two `PerlinNoise` samplers with slightly different coordinate scaling
//! to create smoother, more natural-looking noise. It's used for biome climate parameters.

use crate::noise::PerlinNoise;
use crate::random::{PositionalRandom, RandomSource, RandomSplitter};

/// Input factor for the second Perlin sampler.
///
/// This is the exact value from vanilla `NormalNoise.java`.
/// The second sampler's coordinates are multiplied by this factor to create
/// variation between the two samplers.
#[allow(clippy::unreadable_literal)]
pub const INPUT_FACTOR: f64 = 1.0181268882175227;

/// Value factor numerator matching vanilla's inline literal `0.16666666666666666` (1/6).
///
/// Vanilla declares a constant `TARGET_DEVIATION = 0.3333333333333333` (1/3) but never
/// uses it — the constructor hardcodes `0.16666666666666666` (1/6) as the numerator in
/// `valueFactor = 0.16666... / expectedDeviation(span)`. We name this differently to
/// avoid confusion with vanilla's dead `TARGET_DEVIATION` constant.
#[allow(clippy::unreadable_literal)]
const VALUE_FACTOR_NUMERATOR: f64 = 0.16666666666666666;

/// Normal (Double Perlin) noise generator.
///
/// Combines two `PerlinNoise` samplers with different coordinate scales to create
/// smoother noise. The result is scaled by a value factor based on the octave span.
#[derive(Debug, Clone)]
pub struct NormalNoise {
    /// First Perlin noise sampler
    first: PerlinNoise,
    /// Second Perlin noise sampler (coordinates scaled by `INPUT_FACTOR`)
    second: PerlinNoise,
    /// Factor applied to the sum of both samplers
    value_factor: f64,
    /// Maximum possible output value
    max_value: f64,
}

impl NormalNoise {
    /// Create a new `NormalNoise` from a mutable sequential random source.
    ///
    /// This matches vanilla's `NormalNoise` constructor:
    /// 1. Create first `PerlinNoise` (which advances the random state by consuming 262 + forking)
    /// 2. Create second `PerlinNoise` (which sees the advanced state)
    ///
    /// This ensures the two `PerlinNoise` instances have different seeds.
    #[must_use]
    pub fn create_from_random(
        random: &mut RandomSource,
        first_octave: i32,
        amplitudes: &[f64],
    ) -> Self {
        let first = PerlinNoise::create_from_random(random, first_octave, amplitudes);
        let second = PerlinNoise::create_from_random(random, first_octave, amplitudes);

        Self::finish(first, second, amplitudes)
    }

    /// Create a new `NormalNoise` from a positional random splitter.
    ///
    /// **Note**: This creates a sequential random source from the splitter's noise ID,
    /// then delegates to `create_from_random` for vanilla-matching behavior.
    #[must_use]
    pub fn create(
        splitter: &RandomSplitter,
        noise_id: &str,
        first_octave: i32,
        amplitudes: &[f64],
    ) -> Self {
        let mut random = splitter.with_hash_of(noise_id);
        Self::create_from_random(&mut random, first_octave, amplitudes)
    }

    /// Create a `NormalNoise` using the legacy nether biome initialization path.
    ///
    /// This uses `PerlinNoise::create_legacy_for_nether` instead of the hash-based
    /// positional seeding. The `ImprovedNoise` instances are created directly from
    /// a sequential `LegacyRandomSource`. Matches vanilla's
    /// `NormalNoise.createLegacyNetherBiome()`.
    #[must_use]
    pub fn create_legacy_nether_biome(
        random: &mut RandomSource,
        first_octave: i32,
        amplitudes: &[f64],
    ) -> Self {
        let first = PerlinNoise::create_legacy_for_nether(random, first_octave, amplitudes);
        let second = PerlinNoise::create_legacy_for_nether(random, first_octave, amplitudes);

        Self::finish(first, second, amplitudes)
    }

    /// Finish construction with the two `PerlinNoise` instances.
    fn finish(first: PerlinNoise, second: PerlinNoise, amplitudes: &[f64]) -> Self {
        // Find the span of non-zero octaves
        let mut min_octave = i32::MAX;
        let mut max_octave = i32::MIN;
        for (i, &amp) in amplitudes.iter().enumerate() {
            if amp != 0.0 {
                min_octave = min_octave.min(i as i32);
                max_octave = max_octave.max(i as i32);
            }
        }

        // All-zero amplitudes: silent noise, always returns 0.
        if min_octave == i32::MAX {
            return Self {
                first,
                second,
                value_factor: 0.0,
                max_value: 0.0,
            };
        }

        // Calculate value factor based on octave span
        let octave_span = max_octave - min_octave;
        let value_factor = VALUE_FACTOR_NUMERATOR / expected_deviation(octave_span);
        let max_value = (first.max_value() + second.max_value()) * value_factor;

        Self {
            first,
            second,
            value_factor,
            max_value,
        }
    }

    /// Sample the noise at the given coordinates.
    ///
    /// The result combines two Perlin noise samples:
    /// - First sampler at (x, y, z)
    /// - Second sampler at (x * `INPUT_FACTOR`, y * `INPUT_FACTOR`, z * `INPUT_FACTOR`)
    ///
    /// The sum is then scaled by the value factor.
    #[inline]
    #[must_use]
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        let x2 = x * INPUT_FACTOR;
        let y2 = y * INPUT_FACTOR;
        let z2 = z * INPUT_FACTOR;
        (self.first.get_value(x, y, z) + self.second.get_value(x2, y2, z2)) * self.value_factor
    }

    /// Get the maximum possible output value.
    #[inline]
    #[must_use]
    pub const fn max_value(&self) -> f64 {
        self.max_value
    }
}

/// Calculate the expected deviation for a given octave span.
///
/// This is used to normalize the output of the combined noise.
/// Formula: 0.1 * (1 + 1/(span + 1))
#[inline]
fn expected_deviation(octave_span: i32) -> f64 {
    0.1 * (1.0 + 1.0 / f64::from(octave_span + 1))
}

#[cfg(test)]
#[allow(clippy::unreadable_literal)]
mod tests {
    use super::*;
    use crate::random::{Random, xoroshiro::Xoroshiro};

    #[test]
    fn test_normal_noise_deterministic() {
        let mut rng = Xoroshiro::from_seed(12345);
        let splitter = rng.next_positional();

        let amplitudes = [1.0, 1.0, 1.0];
        let noise1 = NormalNoise::create(&splitter, "test_noise", -3, &amplitudes);
        let noise2 = NormalNoise::create(&splitter, "test_noise", -3, &amplitudes);

        let v1 = noise1.get_value(100.0, 64.0, 100.0);
        let v2 = noise2.get_value(100.0, 64.0, 100.0);
        assert!((v1 - v2).abs() < 1e-15);
    }

    #[test]
    fn test_normal_noise_spatial_variation() {
        let mut rng = Xoroshiro::from_seed(42);
        let splitter = rng.next_positional();

        let noise = NormalNoise::create(&splitter, "test_noise", -4, &[1.0, 1.0, 1.0, 1.0]);

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
    fn test_first_and_second_differ() {
        let mut rng = Xoroshiro::from_seed(12345);
        let splitter = rng.next_positional();

        let noise = NormalNoise::create(&splitter, "test_noise", -3, &[1.0, 1.0, 1.0]);

        // The first and second samplers should produce different raw values
        // (but we can only test via the combined output)
        let v1 = noise.get_value(1000.0, 0.0, 1000.0);
        let v2 = noise.get_value(1001.0, 0.0, 1000.0);
        // Values at different coordinates should differ
        assert!((v1 - v2).abs() > 0.0001);
    }

    #[test]
    fn test_expected_deviation() {
        // Check the formula produces expected values
        assert!((expected_deviation(0) - 0.2).abs() < 1e-10);
        assert!((expected_deviation(1) - 0.15).abs() < 1e-10);
        assert!((expected_deviation(2) - 0.13333333333333333).abs() < 1e-10);
    }

    #[test]
    fn test_input_factor() {
        // Verify the constant matches vanilla
        assert!((INPUT_FACTOR - 1.0181268882175227).abs() < 1e-15);
    }
}
