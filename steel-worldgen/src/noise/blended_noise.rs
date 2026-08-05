//! `BlendedNoise` implementation matching vanilla Minecraft's `BlendedNoise.java`
//!
//! Combines three `PerlinNoise` instances (min limit, max limit, main) for terrain generation.
//! The main noise determines the blend factor between the min and max limit noises.

use std::simd::Simd;
use std::simd::cmp::SimdPartialOrd;

use crate::noise::PerlinNoise;
use crate::random::RandomSource;
use steel_math::{clamped_lerp, clamped_lerp_simd, wrap, wrap_simd};

/// Base frequency multiplier for all `BlendedNoise` coordinate transforms.
const COORDINATE_SCALE: f64 = 684.412;

/// Runtime `BlendedNoise` sampler with three seeded `PerlinNoise` instances.
///
/// Matches vanilla's `BlendedNoise` density function.
#[derive(Debug, Clone)]
pub struct BlendedNoise {
    min_limit_noise: PerlinNoise,
    max_limit_noise: PerlinNoise,
    main_noise: PerlinNoise,
    xz_multiplier: f64,
    y_multiplier: f64,
    xz_factor: f64,
    y_factor: f64,
    smear_scale_multiplier: f64,
    max_value: f64,
}

impl BlendedNoise {
    /// Create a new `BlendedNoise` from a random source and scale parameters.
    ///
    /// This matches vanilla's `BlendedNoise(RandomSource, ...)` constructor which
    /// uses the legacy initialization path with `createLegacyForBlendedNoise`.
    #[must_use]
    pub fn new(
        random: &mut RandomSource,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> Self {
        // min/max limit: 16 octaves (-15 to 0), main: 8 octaves (-7 to 0)
        let min_limit_noise = PerlinNoise::create_legacy_for_nether(random, -15, &[1.0; 16]);
        let max_limit_noise = PerlinNoise::create_legacy_for_nether(random, -15, &[1.0; 16]);
        let main_noise = PerlinNoise::create_legacy_for_nether(random, -7, &[1.0; 8]);

        let xz_multiplier = COORDINATE_SCALE * xz_scale;
        let y_multiplier = COORDINATE_SCALE * y_scale;
        let max_value = min_limit_noise.max_broken_value(y_multiplier);

        Self {
            min_limit_noise,
            max_limit_noise,
            main_noise,
            xz_multiplier,
            y_multiplier,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
            max_value,
        }
    }

    /// Compute the blended noise value at the given block coordinates.
    #[must_use]
    pub fn compute(&self, block_x: f64, block_y: f64, block_z: f64) -> f64 {
        let limit_x = block_x * self.xz_multiplier;
        let limit_y = block_y * self.y_multiplier;
        let limit_z = block_z * self.xz_multiplier;
        let main_x = limit_x / self.xz_factor;
        let main_y = limit_y / self.y_factor;
        let main_z = limit_z / self.xz_factor;
        let limit_smear = self.y_multiplier * self.smear_scale_multiplier;
        let main_smear = limit_smear / self.y_factor;

        // Sample main noise (8 octaves, highest frequency first)
        let mut main_noise_value = 0.0;
        let mut pow = 1.0;
        for i in 0..8 {
            if let Some(noise) = self.main_noise.get_octave_noise(i) {
                main_noise_value += noise.noise_with_y_scale(
                    wrap(main_x * pow),
                    wrap(main_y * pow),
                    wrap(main_z * pow),
                    main_smear * pow,
                    main_y * pow,
                ) / pow;
            }
            pow /= 2.0;
        }

        // Determine blend factor and which limit noises to sample
        let factor = f64::midpoint(main_noise_value / 10.0, 1.0);
        let is_max = factor >= 1.0;
        let is_min = factor <= 0.0;

        // Sample limit noises (16 octaves each, highest frequency first)
        let mut blend_min = 0.0;
        let mut blend_max = 0.0;
        pow = 1.0;
        for i in 0..16 {
            let wx = wrap(limit_x * pow);
            let wy = wrap(limit_y * pow);
            let wz = wrap(limit_z * pow);
            let y_scale_pow = limit_smear * pow;

            if !is_max && let Some(noise) = self.min_limit_noise.get_octave_noise(i) {
                blend_min += noise.noise_with_y_scale(wx, wy, wz, y_scale_pow, limit_y * pow) / pow;
            }

            if !is_min && let Some(noise) = self.max_limit_noise.get_octave_noise(i) {
                blend_max += noise.noise_with_y_scale(wx, wy, wz, y_scale_pow, limit_y * pow) / pow;
            }

            pow /= 2.0;
        }

        clamped_lerp(blend_min / 512.0, blend_max / 512.0, factor) / 128.0
    }

    /// Compute blended noise for N points sharing the same (x, z) but with
    /// different y values. Returns results as an array.
    ///
    /// This uses SIMD to vectorize the math-heavy portions (gradient dots,
    /// smoothstep, trilinear lerp) across the N Y lanes, while sharing
    /// the x/z coordinate work.
    #[inline]
    #[must_use]
    pub fn compute_simd<const N: usize>(
        &self,
        block_x: f64,
        block_ys: [f64; N],
        block_z: f64,
    ) -> [f64; N] {
        let limit_x = block_x * self.xz_multiplier;
        let limit_ys = Simd::from_array(block_ys) * Simd::splat(self.y_multiplier);
        let limit_z = block_z * self.xz_multiplier;
        let main_x = limit_x / self.xz_factor;
        let main_ys = limit_ys / Simd::splat(self.y_factor);
        let main_z = limit_z / self.xz_factor;
        let limit_smear = self.y_multiplier * self.smear_scale_multiplier;
        let main_smear = limit_smear / self.y_factor;

        let mut main_noise_values = Simd::splat(0.0);
        let mut pow = 1.0;
        for i in 0..8 {
            if let Some(noise) = self.main_noise.get_octave_noise(i) {
                let pow_v = Simd::splat(pow);
                let scaled_ys = main_ys * pow_v;
                main_noise_values += noise.noise_with_y_scale_simd(
                    wrap(main_x * pow),
                    wrap_simd(scaled_ys),
                    wrap(main_z * pow),
                    main_smear * pow,
                    scaled_ys,
                ) / pow_v;
            }
            pow /= 2.0;
        }

        let factors = (main_noise_values / Simd::splat(10.0) + Simd::splat(1.0)) / Simd::splat(2.0);

        let all_max = factors.simd_ge(Simd::splat(1.0)).all();
        let all_min = factors.simd_le(Simd::splat(0.0)).all();

        let mut blend_min = Simd::splat(0.0);
        let mut blend_max = Simd::splat(0.0);
        pow = 1.0;
        for i in 0..16 {
            let pow_v = Simd::splat(pow);
            let scaled_ys = limit_ys * pow_v;
            let wx = wrap(limit_x * pow);
            let wys = wrap_simd(scaled_ys);
            let wz = wrap(limit_z * pow);
            let y_scale_pow = limit_smear * pow;

            if !all_max && let Some(noise) = self.min_limit_noise.get_octave_noise(i) {
                blend_min +=
                    noise.noise_with_y_scale_simd(wx, wys, wz, y_scale_pow, scaled_ys) / pow_v;
            }

            if !all_min && let Some(noise) = self.max_limit_noise.get_octave_noise(i) {
                blend_max +=
                    noise.noise_with_y_scale_simd(wx, wys, wz, y_scale_pow, scaled_ys) / pow_v;
            }

            pow /= 2.0;
        }

        let min_scaled = blend_min / Simd::splat(512.0);
        let max_scaled = blend_max / Simd::splat(512.0);
        let result = clamped_lerp_simd(min_scaled, max_scaled, factors) / Simd::splat(128.0);
        result.to_array()
    }

    /// Compute blended noise for a column of Y values, returning the results.
    ///
    /// Uses SIMD to process 4 Y values at a time.
    pub fn compute_column(&self, block_x: i32, block_ys: &[i32], block_z: i32, out: &mut [f64]) {
        let count = block_ys.len().min(out.len());
        let block_x = f64::from(block_x);
        let block_z = f64::from(block_z);
        let mut processed = 0;

        // SIMD batches of 4
        let chunks_4 = count / 4;
        for chunk in 0..chunks_4 {
            let base = chunk * 4;
            let batch_ys = [
                f64::from(block_ys[base]),
                f64::from(block_ys[base + 1]),
                f64::from(block_ys[base + 2]),
                f64::from(block_ys[base + 3]),
            ];
            out[base..base + 4].copy_from_slice(&self.compute_simd(block_x, batch_ys, block_z));
        }
        processed += chunks_4 * 4;

        // SIMD batches of 2 (max 1 chunk possible after chunks of 4)
        if count - processed >= 2 {
            let batch_ys = [
                f64::from(block_ys[processed]),
                f64::from(block_ys[processed + 1]),
            ];
            out[processed..processed + 2]
                .copy_from_slice(&self.compute_simd(block_x, batch_ys, block_z));
            processed += 2;
        }

        // Scalar remainder (handles the final 0 or 1 element)
        for i in processed..count {
            out[i] = self.compute(block_x, f64::from(block_ys[i]), block_z);
        }
    }

    /// Maximum possible output value.
    #[inline]
    #[must_use]
    pub const fn max_value(&self) -> f64 {
        self.max_value
    }

    /// Minimum possible output value (negative of max).
    #[inline]
    #[must_use]
    pub fn min_value(&self) -> f64 {
        -self.max_value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::xoroshiro::Xoroshiro;

    fn make_source(seed: u64) -> RandomSource {
        RandomSource::Xoroshiro(Xoroshiro::from_seed(seed))
    }

    #[test]
    fn test_compute_4x_matches_scalar() {
        let bn = BlendedNoise::new(&mut make_source(42), 1.0, 1.0, 80.0, 160.0, 8.0);

        // Test column of Y values at various (x, z)
        let test_cases: &[(f64, [f64; 4], f64)] = &[
            (0., [0., 8., 16., 24.], 0.),
            (16., [32., 40., 48., 56.], 16.),
            (-8., [-64., -32., 0., 32.], 8.),
            (100., [60., 64., 68., 72.], -50.),
            (0., [-4., -2., 0., 2.], 0.),
        ];

        for &(x, ys, z) in test_cases {
            let simd = bn.compute_simd(x, ys, z);
            for i in 0..4 {
                let scalar = bn.compute(x, ys[i], z);
                assert!(
                    (scalar - simd[i]).abs() < 1e-12,
                    "Mismatch at ({x}, {}, {z}): scalar={scalar}, simd={}, diff={}",
                    ys[i],
                    simd[i],
                    (scalar - simd[i]).abs(),
                );
            }
        }
    }

    #[test]
    fn test_compute_column_matches_scalar() {
        let bn = BlendedNoise::new(&mut make_source(42), 1.0, 1.0, 80.0, 160.0, 8.0);

        // 49 Y values like the actual overworld (cell_min_y=-8, corners_y=49, cell_height=8)
        let block_ys: Vec<i32> = (0..49).map(|cy| (cy - 8) * 8).collect();

        let scalar_results: Vec<f64> = block_ys
            .iter()
            .map(|&y| bn.compute(0., f64::from(y), 0.))
            .collect();

        let mut column_results = vec![0.0; block_ys.len()];
        bn.compute_column(0, &block_ys, 0, &mut column_results);

        for (i, &y) in block_ys.iter().enumerate() {
            assert!(
                (scalar_results[i] - column_results[i]).abs() < 1e-12,
                "Column mismatch at y={y}: scalar={}, column={}, diff={}",
                scalar_results[i],
                column_results[i],
                (scalar_results[i] - column_results[i]).abs(),
            );
        }
    }

    #[test]
    fn test_blended_noise_deterministic() {
        let bn1 = BlendedNoise::new(&mut make_source(12345), 1.0, 1.0, 80.0, 160.0, 8.0);
        let bn2 = BlendedNoise::new(&mut make_source(12345), 1.0, 1.0, 80.0, 160.0, 8.0);

        let v1 = bn1.compute(0., 64., 0.);
        let v2 = bn2.compute(0., 64., 0.);
        assert!(
            (v1 - v2).abs() < 1e-15,
            "BlendedNoise not deterministic: {v1} vs {v2}",
        );
    }

    #[test]
    fn test_blended_noise_spatial_variation() {
        let bn = BlendedNoise::new(&mut make_source(42), 1.0, 1.0, 80.0, 160.0, 8.0);

        let values: Vec<f64> = (-5..5)
            .map(|x| bn.compute(f64::from(x * 16), 64., 0.))
            .collect();

        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max - min > 1e-6,
            "BlendedNoise should have variation: {values:?}"
        );
    }

    #[test]
    fn test_blended_noise_range() {
        let bn = BlendedNoise::new(&mut make_source(42), 1.0, 1.0, 80.0, 160.0, 8.0);

        for x in -10..10 {
            for y in (-4..20).step_by(4) {
                let v = bn.compute(f64::from(x * 16), f64::from(y * 4), f64::from(x * 16));
                assert!(
                    v.abs() <= bn.max_value() + 0.01,
                    "BlendedNoise value {v} exceeds max {} at ({}, {}, {})",
                    bn.max_value(),
                    x * 16,
                    y * 4,
                    x * 16,
                );
            }
        }
    }
}
