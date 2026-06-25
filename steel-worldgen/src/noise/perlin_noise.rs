//! Octave-based Perlin noise implementation matching vanilla Minecraft's `PerlinNoise.java`
//!
//! This combines multiple `ImprovedNoise` instances at different frequencies (octaves)
//! to create more natural-looking noise with detail at multiple scales.

use std::ops;
use std::simd::cmp::{SimdPartialEq, SimdPartialOrd};
use std::simd::num::SimdFloat;
use std::simd::{Mask, Simd, SimdCast, SimdElement, StdFloat, f64x4};

use crate::noise::ImprovedNoise;
use crate::random::{PositionalRandom, Random, RandomSource, RandomSplitter, name_hash::NameHash};
use steel_math::{wrap, wrap_simd};

/// Octave-based Perlin noise generator.
///
/// Combines multiple [`ImprovedNoise`] instances at different frequencies
/// to create noise with detail at multiple scales.
#[derive(Debug, Clone)]
pub struct PerlinNoise {
    /// Noise generators for each octave (None if amplitude is 0).
    /// Kept for [`Self::get_octave_noise`] which is indexed by original octave order.
    noise_levels: Vec<Option<ImprovedNoise>>,
    /// Amplitude multipliers for each octave.
    /// Kept for [`Self::max_broken_value`] which recomputes the edge value.
    amplitudes: Vec<f64>,
    /// Pre-computed per-octave data for the hot sampling loop. Only contains
    /// entries with non-zero amplitude, in original (low → high frequency) order
    /// so that floating-point accumulation matches the legacy iteration.
    active_octaves: Vec<ActiveOctave>,
    /// Factor applied to output values for the lowest frequency octave.
    /// Used by [`Self::max_broken_value`] which recomputes the edge value.
    lowest_freq_value_factor: f64,
    /// Maximum possible output value
    max_value: f64,
}

/// Pre-computed octave data for the hot sampling path.
///
/// Avoids per-iteration `*= 2.0` / `/= 2.0` factor reductions and the per-octave
/// `Option` check in [`PerlinNoise::get_value_with_y_params`].
#[derive(Debug, Clone)]
struct ActiveOctave {
    noise: ImprovedNoise,
    /// `lowest_freq_input_factor * 2^i` for original octave index `i`.
    input_factor: f64,
    /// `amplitude * (lowest_freq_value_factor / 2^i)` — combines the per-octave
    /// amplitude with the value factor reduction so the inner loop does one
    /// multiply + one add per octave.
    output_factor: f64,
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
                let mut octave_random = splitter.with_hash_of(&NameHash::new(&name));
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
                let mut octave_random = splitter.with_hash_of(&NameHash::new(&name));
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

        if zero_octave_index < octaves && amplitudes[zero_octave_index] != 0.0 {
            noise_levels[zero_octave_index] = Some(ImprovedNoise::new(random));
        } else {
            random.consume_count(262);
        }

        for ix in (0..zero_octave_index).rev() {
            if ix < octaves && amplitudes[ix] != 0.0 {
                noise_levels[ix] = Some(ImprovedNoise::new(random));
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

        // Pre-compute per-octave factors for the hot path. Mirrors the legacy
        // iteration order so summation is bit-identical.
        let mut active_octaves = Vec::with_capacity(noise_levels.len());
        let mut input_factor = lowest_freq_input_factor;
        let mut value_factor = lowest_freq_value_factor;
        for (i, noise_opt) in noise_levels.iter().enumerate() {
            if let Some(noise) = noise_opt {
                active_octaves.push(ActiveOctave {
                    noise: noise.clone(),
                    input_factor,
                    output_factor: amplitudes[i] * value_factor,
                });
            }
            input_factor *= 2.0;
            value_factor /= 2.0;
        }

        Self {
            noise_levels,
            amplitudes: amplitudes.to_vec(),
            active_octaves,
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
        let mut value = 0.0;

        for octave in &self.active_octaves {
            let input_factor = octave.input_factor;
            let noise_val = octave.noise.noise(
                wrap(x * input_factor),
                wrap(y * input_factor),
                wrap(z * input_factor),
            );
            value += octave.output_factor * noise_val;
        }

        value
    }

    /// Sample the noise at `(x, 0.0, z)`.
    #[inline]
    #[must_use]
    pub fn get_value_xz(&self, x: f64, z: f64) -> f64 {
        let mut value = 0.0;

        for octave in &self.active_octaves {
            let input_factor = octave.input_factor;
            let noise_val = octave
                .noise
                .noise_xz(wrap(x * input_factor), wrap(z * input_factor));
            value += octave.output_factor * noise_val;
        }

        value
    }

    /// Sample the noise at `(x, y, 0.0)`.
    #[inline]
    #[must_use]
    pub fn get_value_xy(&self, x: f64, y: f64) -> f64 {
        let mut value = 0.0;

        for octave in &self.active_octaves {
            let input_factor = octave.input_factor;
            let noise_val = octave
                .noise
                .noise_xy(wrap(x * input_factor), wrap(y * input_factor));
            value += octave.output_factor * noise_val;
        }

        value
    }

    /// Calculate Perlin noise value using SIMD vectors.
    #[inline]
    #[must_use]
    pub fn get_value_simd<F, const N: usize>(
        &self,
        x: Simd<F, N>,
        y: Simd<F, N>,
        z: Simd<F, N>,
    ) -> Simd<F, N>
    where
        F: SimdElement + SimdCast,
        Simd<F, N>: SimdFloat<Cast<i32> = Simd<i32, N>>
            + SimdPartialOrd
            + SimdPartialEq<Mask = Mask<<F as SimdElement>::Mask, N>>
            + ops::Add<Output = Simd<F, N>>
            + ops::Sub<Output = Simd<F, N>>
            + ops::Mul<Output = Simd<F, N>>
            + ops::Div<Output = Simd<F, N>>
            + ops::Neg<Output = Simd<F, N>>
            + StdFloat,
    {
        let mut value = Simd::splat(0.0).cast();

        for octave in &self.active_octaves {
            let input_factor = Simd::splat(octave.input_factor).cast();
            let noise_val = octave.noise.noise_simd(
                wrap_simd(x * input_factor),
                wrap_simd(y * input_factor),
                wrap_simd(z * input_factor),
            );
            value += Simd::splat(octave.output_factor).cast() * noise_val;
        }

        value
    }

    /// Sample the noise with Y scaling parameters.
    ///
    /// # Arguments
    /// * `x`, `y`, `z` - Coordinates to sample
    /// * `y_scale` - Y scaling factor for terrain
    /// * `y_fudge` - Y fudge factor for floor snapping
    /// * `y_flat_hack` - If true, use `-yo` instead of wrapped y (for legacy biomes)
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

        for octave in &self.active_octaves {
            let input_factor = octave.input_factor;
            let noise = &octave.noise;
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
            value += octave.output_factor * noise_val;
        }

        value
    }

    /// SIMD form of [`Self::get_value_with_y_params`] that processes 4 Y values
    /// at a fixed `(x, z)` per call. Used by transpiled density-function trees
    /// that batch 4 cell-corner Ys in one pass.
    ///
    /// Per-lane math is identical to the scalar path — same operation order,
    /// same wrapping, same octave loop — so the 4 returned lanes are
    /// bit-identical to four scalar calls at the same Y values.
    #[must_use]
    pub fn get_value_with_y_params_4x(
        &self,
        x: f64,
        ys: f64x4,
        z: f64,
        y_scale: f64,
        y_fudge: f64,
        y_flat_hack: bool,
    ) -> f64x4 {
        let mut value = f64x4::splat(0.0);

        for octave in &self.active_octaves {
            let input_factor = octave.input_factor;
            let noise = &octave.noise;
            let x_w = wrap(x * input_factor);
            let z_w = wrap(z * input_factor);
            let ys_for_call = if y_flat_hack {
                f64x4::splat(-noise.yo)
            } else {
                wrap_simd(ys * f64x4::splat(input_factor))
            };
            let y_fudges = f64x4::splat(y_fudge * input_factor);
            let noise_val = noise.noise_with_y_scale_simd(
                x_w,
                ys_for_call,
                z_w,
                y_scale * input_factor,
                y_fudges,
            );
            value += f64x4::splat(octave.output_factor) * noise_val;
        }

        value
    }

    /// Generic N-lane form of [`Self::get_value_with_y_params_4x`]. Per-lane
    /// math is identical to the scalar path at any supported width.
    #[must_use]
    pub fn get_value_with_y_params_simd<const N: usize>(
        &self,
        x: f64,
        ys: Simd<f64, N>,
        z: f64,
        y_scale: f64,
        y_fudge: f64,
        y_flat_hack: bool,
    ) -> Simd<f64, N> {
        let mut value = Simd::splat(0.0);

        for octave in &self.active_octaves {
            let input_factor = octave.input_factor;
            let noise = &octave.noise;
            let x_w = wrap(x * input_factor);
            let z_w = wrap(z * input_factor);
            let ys_for_call = if y_flat_hack {
                Simd::splat(-noise.yo)
            } else {
                wrap_simd(ys * Simd::splat(input_factor))
            };
            let y_fudges = Simd::splat(y_fudge * input_factor);
            let noise_val = noise.noise_with_y_scale_simd(
                x_w,
                ys_for_call,
                z_w,
                y_scale * input_factor,
                y_fudges,
            );
            value += Simd::splat(octave.output_factor) * noise_val;
        }

        value
    }

    /// Get the maximum possible output value.
    #[inline]
    #[must_use]
    pub const fn max_value(&self) -> f64 {
        self.max_value
    }

    /// Calculate the maximum "broken" value for `BlendedNoise`.
    ///
    /// Used by `BlendedNoise` to determine the theoretical max output.
    /// Java reference: `PerlinNoise.maxBrokenValue(double)`
    #[must_use]
    pub fn max_broken_value(&self, y_scale: f64) -> f64 {
        Self::edge_value(
            &self.amplitudes,
            self.lowest_freq_value_factor,
            y_scale + 2.0,
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::{Random, xoroshiro::Xoroshiro};
    use std::simd::f64x4;

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
    fn test_get_value_matches_zero_y_params_path() {
        let mut rng = Xoroshiro::from_seed(12345);
        let splitter = rng.next_positional();

        let noise = PerlinNoise::create(&splitter, -4, &[1.0, 0.0, 1.0, 1.0]);

        for (x, y, z) in [
            (0.0, 0.0, 0.0),
            (100.0, 64.0, -100.0),
            (-4096.25, -32.5, 1024.75),
        ] {
            assert!(
                (noise.get_value(x, y, z)
                    - noise.get_value_with_y_params(x, y, z, 0.0, 0.0, false))
                .abs()
                    < 1e-15
            );
        }
    }

    #[test]
    fn test_get_value_simd_matches_scalar() {
        let mut rng = Xoroshiro::from_seed(12_345);
        let splitter = rng.next_positional();
        let noise = PerlinNoise::create(&splitter, -6, &[1.0, 0.0, 1.0, 1.0, 0.5]);
        let xs = [0.0, 1.25, -1000.0, 33_554_431.5];
        let ys = [0.0, 64.5, -32.25, 255.75];
        let zs = [0.0, -30.75, 4096.5, -33_554_432.25];

        let simd = noise.get_value_simd(
            f64x4::from_array(xs),
            f64x4::from_array(ys),
            f64x4::from_array(zs),
        );

        for i in 0..4 {
            let scalar = noise.get_value(xs[i], ys[i], zs[i]);
            #[expect(
                clippy::float_cmp,
                reason = "SIMD path must be bit-identical to scalar noise for vanilla determinism"
            )]
            let matches = scalar == simd[i];
            assert!(
                matches,
                "Mismatch at ({}, {}, {}): scalar={}, simd={}",
                xs[i], ys[i], zs[i], scalar, simd[i],
            );
        }
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
        let mut random = splitter.with_hash_of(&NameHash::new("test_noise"));

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
    fn test_zero_axis_helpers_match_full_noise() {
        let mut rng = Xoroshiro::from_seed(98_765);
        let splitter = rng.next_positional();
        let noise = PerlinNoise::create(&splitter, -6, &[1.0, 0.0, 1.0, 1.0, 0.5]);
        let samples = [
            (0.0, 0.0),
            (1.25, -30.75),
            (-1000.0, 4096.5),
            (33_554_431.5, -33_554_432.25),
            (-0.000_000_1, 0.000_000_1),
        ];

        for &(a, b) in &samples {
            #[expect(
                clippy::float_cmp,
                reason = "zero-axis helpers must be bit-identical to the full scalar path"
            )]
            {
                assert_eq!(noise.get_value_xz(a, b), noise.get_value(a, 0.0, b));
                assert_eq!(noise.get_value_xy(a, b), noise.get_value(a, b, 0.0));
            }
        }
    }
}
