//! Normal (Double Perlin) noise implementation matching vanilla Minecraft's NormalNoise.java
//!
//! This combines two `PerlinNoise` samplers with slightly different coordinate scaling
//! to create smoother, more natural-looking noise. It's used for biome climate parameters.

use std::ops;
use std::simd::cmp::{SimdPartialEq, SimdPartialOrd};
use std::simd::f64x4;
use std::simd::num::SimdFloat;
use std::simd::{Mask, Simd, SimdCast, SimdElement, StdFloat};

use crate::noise::PerlinNoise;
use crate::random::{PositionalRandom, RandomSource, RandomSplitter, name_hash::NameHash};

/// Input factor for the second Perlin sampler.
///
/// This is the exact value from vanilla `NormalNoise.java`.
/// The second sampler's coordinates are multiplied by this factor to create
/// variation between the two samplers.
#[expect(
    clippy::unreadable_literal,
    reason = "exact vanilla constant; underscores would obscure precision"
)]
pub const INPUT_FACTOR: f64 = 1.0181268882175227;

/// `NormalNoise.TARGET_DEVIATION` — target standard deviation for the combined output.
const TARGET_DEVIATION: f64 = 0.3333333333333333;

/// Per-octave deviation coefficient used by `estimateDeviation`.
///
/// Vanilla's `NormalNoise.estimateDeviation`: each octave layer contributes
/// `0.2702247831245211 * octave.absAmplitude()` to the variance sum.
#[expect(
    clippy::unreadable_literal,
    reason = "exact vanilla constant; underscores would obscure precision"
)]
const DEVIATION_COEFFICIENT: f64 = 0.2702247831245211;

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

        Self::finish(first, second, parity_value_factor(first_octave, amplitudes))
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
        let mut random = splitter.with_hash_of(&NameHash::new(noise_id));
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

        Self::finish(first, second, parity_value_factor(first_octave, amplitudes))
    }

    /// Create a new `NormalNoise` directly from vanilla's current `NormalNoise.Parameters`
    /// codec fields (`base_amplitude`/`base_octave`/`octave_count`/`normalize`/
    /// `amplitude_modifiers`), as used by datapack `worldgen/noise/*.json` entries and
    /// feature-embedded noise providers.
    #[must_use]
    pub fn create_with_params(
        splitter: &RandomSplitter,
        noise_id: &str,
        base_octave: i32,
        base_amplitude: f64,
        octave_count: i32,
        normalize: bool,
        amplitude_modifiers: &[f64],
    ) -> Self {
        let mut random = splitter.with_hash_of(&NameHash::new(noise_id));
        Self::create_from_random_with_params(
            &mut random,
            base_octave,
            base_amplitude,
            octave_count,
            normalize,
            amplitude_modifiers,
        )
    }

    /// Create a new `NormalNoise` from a mutable sequential random source, directly from
    /// vanilla's current `NormalNoise.Parameters` codec fields. See [`Self::create_with_params`].
    #[must_use]
    pub fn create_from_random_with_params(
        random: &mut RandomSource,
        base_octave: i32,
        base_amplitude: f64,
        octave_count: i32,
        normalize: bool,
        amplitude_modifiers: &[f64],
    ) -> Self {
        let amplitudes = expand_amplitude_modifiers(amplitude_modifiers, octave_count);
        let first = PerlinNoise::create_from_random(random, base_octave, &amplitudes);
        let second = PerlinNoise::create_from_random(random, base_octave, &amplitudes);
        let value_factor = compute_value_factor(
            base_octave,
            base_amplitude,
            octave_count,
            normalize,
            amplitude_modifiers,
        );

        Self::finish(first, second, value_factor)
    }

    /// Finish construction with the two `PerlinNoise` instances and a precomputed value factor.
    fn finish(first: PerlinNoise, second: PerlinNoise, value_factor: f64) -> Self {
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

    /// Calculate normal noise value using SIMD vectors.
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
        let x2 = x * Simd::splat(INPUT_FACTOR).cast::<F>();
        let y2 = y * Simd::splat(INPUT_FACTOR).cast::<F>();
        let z2 = z * Simd::splat(INPUT_FACTOR).cast::<F>();
        (self.first.get_value_simd(x, y, z) + self.second.get_value_simd(x2, y2, z2))
            * Simd::splat(self.value_factor).cast()
    }

    /// Sample the noise at `(x, 0.0, z)`.
    #[inline]
    #[must_use]
    pub fn get_value_xz(&self, x: f64, z: f64) -> f64 {
        let x2 = x * INPUT_FACTOR;
        let z2 = z * INPUT_FACTOR;
        (self.first.get_value_xz(x, z) + self.second.get_value_xz(x2, z2)) * self.value_factor
    }

    /// Sample the noise at `(x, y, 0.0)`.
    #[inline]
    #[must_use]
    pub fn get_value_xy(&self, x: f64, y: f64) -> f64 {
        let x2 = x * INPUT_FACTOR;
        let y2 = y * INPUT_FACTOR;
        (self.first.get_value_xy(x, y) + self.second.get_value_xy(x2, y2)) * self.value_factor
    }

    /// Sample 4 Y values at fixed `(x, z)` in one call.
    #[inline]
    #[must_use]
    pub fn get_value_y_4x(&self, x: f64, ys: f64x4, z: f64) -> f64x4 {
        let x2 = x * INPUT_FACTOR;
        let ys2 = ys * f64x4::splat(INPUT_FACTOR);
        let z2 = z * INPUT_FACTOR;
        (self
            .first
            .get_value_with_y_params_4x(x, ys, z, 0.0, 0.0, false)
            + self
                .second
                .get_value_with_y_params_4x(x2, ys2, z2, 0.0, 0.0, false))
            * f64x4::splat(self.value_factor)
    }

    /// Sample N Y values at fixed `(x, z)` in one call.
    ///
    /// SIMD form of [`Self::get_value`] for transpiled density-function trees
    /// that batch N cell-corner Ys together. Per-lane math is identical to
    /// the scalar path, so `get_value_y_simd(x, splat(y), z)[i] == get_value(x, y, z)`
    /// for any finite `y`.
    #[inline]
    #[must_use]
    pub fn get_value_y_simd<const N: usize>(
        &self,
        x: f64,
        ys: Simd<f64, N>,
        z: f64,
    ) -> Simd<f64, N> {
        let x2 = x * INPUT_FACTOR;
        let ys2 = ys * Simd::splat(INPUT_FACTOR);
        let z2 = z * INPUT_FACTOR;
        (self
            .first
            .get_value_with_y_params_simd::<N>(x, ys, z, 0.0, 0.0, false)
            + self
                .second
                .get_value_with_y_params_simd::<N>(x2, ys2, z2, 0.0, 0.0, false))
            * Simd::splat(self.value_factor)
    }

    /// Get the maximum possible output value.
    #[inline]
    #[must_use]
    pub const fn max_value(&self) -> f64 {
        self.max_value
    }
}

/// `NormalNoise.getAmplitudeModifier`: modifier for octave `index`, or `1.0` when the
/// list is empty (meaning "unmodified").
#[inline]
fn get_amplitude_modifier(amplitude_modifiers: &[f64], index: usize) -> f64 {
    if amplitude_modifiers.is_empty() {
        1.0
    } else {
        amplitude_modifiers[index]
    }
}

/// Expand `amplitude_modifiers` (possibly empty, meaning "all `1.0`") to exactly
/// `octave_count` entries, for feeding into [`PerlinNoise::create_from_random`].
fn expand_amplitude_modifiers(amplitude_modifiers: &[f64], octave_count: i32) -> Vec<f64> {
    (0..octave_count)
        .map(|i| get_amplitude_modifier(amplitude_modifiers, i as usize))
        .collect()
}

/// The persistence-normalization constant vanilla applies when `normalize` is enabled:
/// `0.5^-(octaveCount-1) / (0.5^-octaveCount - 1)`, i.e. `2^(n-1) / (2^n - 1)`.
///
/// This is also, not coincidentally, exactly [`PerlinNoise`]'s own internal
/// `lowest_freq_value_factor` for an `n`-octave amplitude list — see
/// [`compute_value_factor`] for why that equivalence is load-bearing.
#[inline]
fn normalize_const(octave_count: i32) -> f64 {
    2.0_f64.powi(octave_count - 1) / (2.0_f64.powi(octave_count) - 1.0)
}

/// `NormalNoise.buildOctaves`: per-octave amplitude (`baseAmplitude * persistence * modifier`)
/// for each octave with a non-zero amplitude modifier, in ascending octave order.
///
/// Only the amplitude is needed here (not the frequency/octave index) — this is used
/// solely to feed [`estimate_deviation`]/[`compute_normalization_factor`]; actual sampling
/// reuses unmodified [`PerlinNoise`], see [`compute_value_factor`].
fn build_octave_amplitudes(
    base_amplitude: f64,
    octave_count: i32,
    normalize: bool,
    amplitude_modifiers: &[f64],
) -> Vec<f64> {
    let mut amplitude = base_amplitude;
    if normalize {
        amplitude *= normalize_const(octave_count);
    }

    let mut octaves = Vec::with_capacity(octave_count as usize);
    for i in 0..octave_count {
        let modifier = get_amplitude_modifier(amplitude_modifiers, i as usize);
        if modifier != 0.0 {
            octaves.push(amplitude * modifier);
        }
        amplitude *= 0.5;
    }
    octaves
}

/// `NormalNoise.estimateDeviation`: RMS of per-octave layer deviations
/// (`DEVIATION_COEFFICIENT * |amplitude|`).
fn estimate_deviation(octave_amplitudes: &[f64]) -> f64 {
    let variance: f64 = octave_amplitudes
        .iter()
        .map(|amplitude| (DEVIATION_COEFFICIENT * amplitude.abs()).powi(2))
        .sum();
    variance.sqrt()
}

/// `NormalNoise.computeNormalizationFactor`: the scalar applied to every octave's
/// amplitude so the combined noise has the vanilla target standard deviation.
fn compute_normalization_factor(target_amplitude: f64, octave_amplitudes: &[f64]) -> f64 {
    let input_deviation = estimate_deviation(octave_amplitudes);
    if input_deviation == 0.0 {
        return 0.0;
    }
    let input_sum_deviation = input_deviation * std::f64::consts::SQRT_2;
    let target_deviation = target_amplitude * TARGET_DEVIATION;
    target_deviation / input_sum_deviation
}

/// The external scalar applied after summing the first/second [`PerlinNoise`] samplers.
///
/// Vanilla's new `NormalNoise` scales each octave individually by
/// `normalizationFactor * octave.amplitude`, where `octave.amplitude` already has
/// `baseAmplitude` and (if `normalize`) [`normalize_const`] baked in. Reused
/// [`PerlinNoise::create_from_random`] instead computes, per octave `i`,
/// `modifier_i * normalize_const(octave_count) * 0.5^i` (it always applies its own
/// internal persistence normalization, unconditionally). Dividing out that
/// unconditional [`normalize_const`] — canceling it back in when `normalize` is
/// actually requested — lets the single scalar returned here, applied once to
/// `first + second` exactly like the old algorithm, reproduce vanilla's per-octave
/// weighting exactly.
fn compute_value_factor(
    base_octave: i32,
    base_amplitude: f64,
    octave_count: i32,
    normalize: bool,
    amplitude_modifiers: &[f64],
) -> f64 {
    let _ = base_octave; // octave index doesn't affect amplitude-only normalization
    let octave_amplitudes =
        build_octave_amplitudes(base_amplitude, octave_count, normalize, amplitude_modifiers);
    let target_amplitude: f64 = octave_amplitudes.iter().map(|a| a.abs()).sum();
    let normalization_factor = compute_normalization_factor(target_amplitude, &octave_amplitudes);
    let unconditional_normalize_correction = if normalize {
        1.0
    } else {
        1.0 / normalize_const(octave_count)
    };
    normalization_factor * base_amplitude * unconditional_normalize_correction
}

/// `NormalNoise.parityExpectedDeviation`: expected deviation formula used by the legacy
/// (pre-`Parameters`) flat-amplitude-list algorithm. Formula: `0.1 * (1 + 1/(span + 1))`.
#[inline]
fn parity_expected_deviation(octave_span: i32) -> f64 {
    0.1 * (1.0 + 1.0 / f64::from(octave_span + 1))
}

/// `NormalNoise.computeParityNormalizationFactor`: the value factor the legacy algorithm
/// would have produced for a flat `amplitudes` list, used by [`compute_parity_base_amplitude`]
/// to find a `base_amplitude` reproducing that legacy result exactly under the new algorithm.
fn compute_parity_normalization_factor(
    base_amplitude: f64,
    octave_count: i32,
    amplitude_modifiers: &[f64],
) -> f64 {
    let mut min_octave = i32::MAX;
    let mut max_octave = i32::MIN;
    for i in 0..octave_count {
        if get_amplitude_modifier(amplitude_modifiers, i as usize) != 0.0 {
            min_octave = min_octave.min(i);
            max_octave = max_octave.max(i);
        }
    }
    base_amplitude * 0.5 * TARGET_DEVIATION / parity_expected_deviation(max_octave - min_octave)
}

/// `NormalNoise.computeParityBaseAmplitude`: the `base_amplitude` for which the new
/// algorithm reproduces the legacy flat-amplitude-list `NormalNoise(firstOctave, amplitudes)`
/// result exactly.
fn compute_parity_base_amplitude(base_octave: i32, amplitudes: &[f64]) -> f64 {
    let _ = base_octave; // octave index doesn't affect amplitude-only normalization
    let octave_count = amplitudes.len() as i32;
    let octave_amplitudes = build_octave_amplitudes(1.0, octave_count, true, amplitudes);
    let target_amplitude: f64 = octave_amplitudes.iter().map(|a| a.abs()).sum();
    let new_normalization_factor = compute_normalization_factor(target_amplitude, &octave_amplitudes);
    if new_normalization_factor == 0.0 {
        return 1.0;
    }
    let old_normalization_factor = compute_parity_normalization_factor(1.0, octave_count, amplitudes);
    old_normalization_factor / new_normalization_factor
}

/// `NormalNoise.createParity`: the value factor for a legacy flat `(firstOctave, amplitudes)`
/// call, computed by finding the parity `base_amplitude` and running it through the new
/// algorithm — bit-for-bit equivalent to the pre-`Parameters` vanilla `NormalNoise`.
fn parity_value_factor(first_octave: i32, amplitudes: &[f64]) -> f64 {
    if amplitudes.is_empty() {
        return 0.0;
    }
    let base_amplitude = compute_parity_base_amplitude(first_octave, amplitudes);
    compute_value_factor(
        first_octave,
        base_amplitude,
        amplitudes.len() as i32,
        true,
        amplitudes,
    )
}

#[cfg(test)]
#[expect(
    clippy::unreadable_literal,
    reason = "test vectors from vanilla; underscores would obscure precision"
)]
mod tests {
    use super::*;
    use crate::random::{Random, xoroshiro::Xoroshiro};
    use std::simd::f64x4;

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
    fn test_get_value_simd_matches_scalar() {
        let mut rng = Xoroshiro::from_seed(98_765);
        let splitter = rng.next_positional();
        let noise = NormalNoise::create(&splitter, "simd_xyz", -6, &[1.0, 0.0, 1.0, 1.0, 0.5]);
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
    fn test_zero_axis_helpers_match_full_noise() {
        let mut rng = Xoroshiro::from_seed(98_765);
        let splitter = rng.next_positional();
        let noise = NormalNoise::create(&splitter, "zero_axis", -6, &[1.0, 0.0, 1.0, 1.0, 0.5]);
        let samples = [
            (0.0, 0.0),
            (1.25, -30.75),
            (-1000.0, 4096.5),
            (33_554_431.5, -33_554_432.25),
            (-0.000_000_1, 0.000_000_1),
        ];

        for &(a, b) in &samples {
            assert_eq!(noise.get_value_xz(a, b), noise.get_value(a, 0.0, b));
            assert_eq!(noise.get_value_xy(a, b), noise.get_value(a, b, 0.0));
        }
    }

    #[test]
    fn test_parity_expected_deviation() {
        // Check the formula produces expected values
        assert!((parity_expected_deviation(0) - 0.2).abs() < 1e-10);
        assert!((parity_expected_deviation(1) - 0.15).abs() < 1e-10);
        assert!((parity_expected_deviation(2) - 0.13333333333333333).abs() < 1e-10);
    }

    #[test]
    fn test_input_factor() {
        // Verify the constant matches vanilla
        assert!((INPUT_FACTOR - 1.0181268882175227).abs() < 1e-15);
    }

    #[test]
    fn test_get_value_4x_matches_scalar() {
        let mut rng = Xoroshiro::from_seed(54321);
        let splitter = rng.next_positional();
        let noise = NormalNoise::create(&splitter, "test_4x", -7, &[1.0; 8]);

        // Various (x, z) and 4-Y batches.
        let test_cases: &[(f64, [f64; 4], f64)] = &[
            (0.0, [0.0, 8.0, 16.0, 24.0], 0.0),
            (12.5, [-5.0, 10.0, 25.0, 40.0], 7.25),
            (-100.5, [64.0, 65.0, 66.0, 67.0], 200.0),
            (1.0, [0.0; 4], -1.0),
        ];

        for &(x, ys, z) in test_cases {
            let ys_v = f64x4::from_array(ys);
            let simd = noise.get_value_y_4x(x, ys_v, z);
            let generic = noise.get_value_y_simd(x, ys_v, z);
            for i in 0..4 {
                let scalar = noise.get_value(x, ys[i], z);
                let simd_val = simd[i];
                let generic_val = generic[i];
                #[expect(
                    clippy::float_cmp,
                    reason = "SIMD/scalar paths must produce bit-identical results for vanilla determinism"
                )]
                let bit_match = scalar == simd_val;
                assert!(
                    bit_match,
                    "Mismatch at x={x}, y={}, z={z}: scalar={scalar}, simd={simd_val}",
                    ys[i]
                );
                #[expect(
                    clippy::float_cmp,
                    reason = "explicit 4x and generic SIMD paths should be equivalent"
                )]
                let generic_match = simd_val == generic_val;
                assert!(
                    generic_match,
                    "Generic mismatch at x={x}, y={}, z={z}: 4x={simd_val}, generic={generic_val}",
                    ys[i]
                );
            }
        }
    }
}
