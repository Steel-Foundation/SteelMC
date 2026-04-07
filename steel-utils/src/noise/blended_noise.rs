//! `BlendedNoise` implementation matching vanilla Minecraft's `BlendedNoise.java`
//!
//! Combines three `PerlinNoise` instances (min limit, max limit, main) for terrain generation.
//! The main noise determines the blend factor between the min and max limit noises.

use std::cell::RefCell;
use std::simd::cmp::SimdPartialOrd;
use std::simd::f64x4;
use std::simd::{Select, StdFloat};

use crate::math::clamped_lerp;
use crate::noise::PerlinNoise;
use crate::noise::perlin_noise::wrap;
use crate::random::RandomSource;

/// Max Y values we can cache per column (49 for overworld, plenty of headroom).
const MAX_COLUMN_CACHE: usize = 64;

/// Thread-local cache for precomputed blended noise column results.
struct BlendedColumnCache {
    valid: bool,
    block_x: i32,
    block_z: i32,
    count: usize,
    block_ys: [i32; MAX_COLUMN_CACHE],
    results: [f64; MAX_COLUMN_CACHE],
}

impl BlendedColumnCache {
    const fn new() -> Self {
        Self {
            valid: false,
            block_x: 0,
            block_z: 0,
            count: 0,
            block_ys: [0; MAX_COLUMN_CACHE],
            results: [0.0; MAX_COLUMN_CACHE],
        }
    }

    fn lookup(&self, block_x: i32, block_y: i32, block_z: i32) -> Option<f64> {
        if !self.valid || self.block_x != block_x || self.block_z != block_z {
            return None;
        }
        // Linear scan — 49 entries max, negligible vs the ~µs compute cost
        for i in 0..self.count {
            if self.block_ys[i] == block_y {
                return Some(self.results[i]);
            }
        }
        None
    }
}

thread_local! {
    static COLUMN_CACHE: RefCell<BlendedColumnCache> = const { RefCell::new(BlendedColumnCache::new()) };
}

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
    ///
    /// If [`precompute_column`] was called for this (x, z) column, returns the
    /// cached result (O(1) lookup). Otherwise computes from scratch.
    #[must_use]
    pub fn compute(&self, block_x: i32, block_y: i32, block_z: i32) -> f64 {
        let cached = COLUMN_CACHE.with(|c| c.borrow().lookup(block_x, block_y, block_z));
        if let Some(val) = cached {
            return val;
        }
        self.compute_scalar(block_x, block_y, block_z)
    }

    /// Scalar computation (no cache check). Used as fallback and by SIMD batch.
    #[must_use]
    fn compute_scalar(&self, block_x: i32, block_y: i32, block_z: i32) -> f64 {
        let limit_x = f64::from(block_x) * self.xz_multiplier;
        let limit_y = f64::from(block_y) * self.y_multiplier;
        let limit_z = f64::from(block_z) * self.xz_multiplier;
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

    /// Compute blended noise for 4 points sharing the same (x, z) but with
    /// different y values. Returns results as an array.
    ///
    /// This uses SIMD to vectorize the math-heavy portions (gradient dots,
    /// smoothstep, trilinear lerp) across the 4 Y lanes, while sharing
    /// the x/z coordinate work.
    #[must_use]
    pub fn compute_4x(&self, block_x: i32, block_ys: [i32; 4], block_z: i32) -> [f64; 4] {
        let limit_x = f64::from(block_x) * self.xz_multiplier;
        let limit_ys = f64x4::from_array(block_ys.map(f64::from)) * f64x4::splat(self.y_multiplier);
        let limit_z = f64::from(block_z) * self.xz_multiplier;
        let main_x = limit_x / self.xz_factor;
        let main_ys = limit_ys / f64x4::splat(self.y_factor);
        let main_z = limit_z / self.xz_factor;
        let limit_smear = self.y_multiplier * self.smear_scale_multiplier;
        let main_smear = limit_smear / self.y_factor;

        // Sample main noise (8 octaves)
        let mut main_noise_values = f64x4::splat(0.0);
        let mut pow = 1.0;
        for i in 0..8 {
            if let Some(noise) = self.main_noise.get_octave_noise(i) {
                let pow_v = f64x4::splat(pow);
                let scaled_ys = main_ys * pow_v;
                main_noise_values += noise.noise_with_y_scale_4x(
                    wrap(main_x * pow),
                    wrap_4x(scaled_ys),
                    wrap(main_z * pow),
                    main_smear * pow,
                    scaled_ys,
                ) / pow_v;
            }
            pow /= 2.0;
        }

        // Blend factor per lane: midpoint(main/10, 1) = (main/10 + 1) / 2
        let factors =
            (main_noise_values / f64x4::splat(10.0) + f64x4::splat(1.0)) / f64x4::splat(2.0);

        // Early exit: skip a limit noise only when ALL 4 lanes agree
        let all_max = factors.simd_ge(f64x4::splat(1.0)).all();
        let all_min = factors.simd_le(f64x4::splat(0.0)).all();

        // Sample limit noises (16 octaves each)
        let mut blend_min = f64x4::splat(0.0);
        let mut blend_max = f64x4::splat(0.0);
        pow = 1.0;
        for i in 0..16 {
            let pow_v = f64x4::splat(pow);
            let scaled_ys = limit_ys * pow_v;
            let wx = wrap(limit_x * pow);
            let wys = wrap_4x(scaled_ys);
            let wz = wrap(limit_z * pow);
            let y_scale_pow = limit_smear * pow;

            if !all_max && let Some(noise) = self.min_limit_noise.get_octave_noise(i) {
                blend_min +=
                    noise.noise_with_y_scale_4x(wx, wys, wz, y_scale_pow, scaled_ys) / pow_v;
            }

            if !all_min && let Some(noise) = self.max_limit_noise.get_octave_noise(i) {
                blend_max +=
                    noise.noise_with_y_scale_4x(wx, wys, wz, y_scale_pow, scaled_ys) / pow_v;
            }

            pow /= 2.0;
        }

        let min_scaled = blend_min / f64x4::splat(512.0);
        let max_scaled = blend_max / f64x4::splat(512.0);
        let result = clamped_lerp_4x(min_scaled, max_scaled, factors) / f64x4::splat(128.0);
        result.to_array()
    }

    /// Pre-compute blended noise for a column of Y values and store in the
    /// column cache. Subsequent calls to [`compute`] with matching (x, z) will
    /// return cached results instead of recomputing.
    ///
    /// Uses SIMD to process 4 Y values at a time.
    pub fn precompute_column(&self, block_x: i32, block_ys: &[i32], block_z: i32) {
        let count = block_ys.len().min(MAX_COLUMN_CACHE);
        let ys = &block_ys[..count];

        COLUMN_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            cache.valid = true;
            cache.block_x = block_x;
            cache.block_z = block_z;
            cache.count = count;
            cache.block_ys[..count].copy_from_slice(ys);

            // SIMD batches of 4
            let full_chunks = count / 4;
            for chunk in 0..full_chunks {
                let base = chunk * 4;
                let batch_ys = [ys[base], ys[base + 1], ys[base + 2], ys[base + 3]];
                let vals = self.compute_4x(block_x, batch_ys, block_z);
                cache.results[base..base + 4].copy_from_slice(&vals);
            }

            // Scalar remainder
            for (i, &y) in ys.iter().enumerate().skip(full_chunks * 4) {
                cache.results[i] = self.compute_scalar(block_x, y, block_z);
            }
        });
    }

    /// Invalidate the thread-local column cache.
    pub fn invalidate_column_cache() {
        COLUMN_CACHE.with(|c| c.borrow_mut().valid = false);
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

/// Wrap 4 coordinates to prevent precision loss (SIMD version of [`wrap`]).
#[inline]
fn wrap_4x(x: f64x4) -> f64x4 {
    let round_off = f64x4::splat(33_554_432.0);
    x - (x / round_off + f64x4::splat(0.5)).floor() * round_off
}

/// Clamped lerp for 4 lanes.
#[inline]
fn clamped_lerp_4x(min: f64x4, max: f64x4, factor: f64x4) -> f64x4 {
    let zero = f64x4::splat(0.0);
    let one = f64x4::splat(1.0);
    let below = factor.simd_lt(zero);
    let above = factor.simd_gt(one);

    // lerp result for the middle case
    let lerped = min + factor * (max - min);

    // Select: below zero → min, above one → max, otherwise → lerped
    let result = below.select(min, lerped);
    above.select(max, result)
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
        let test_cases: &[(i32, [i32; 4], i32)] = &[
            (0, [0, 8, 16, 24], 0),
            (16, [32, 40, 48, 56], 16),
            (-8, [-64, -32, 0, 32], 8),
            (100, [60, 64, 68, 72], -50),
            (0, [-4, -2, 0, 2], 0),
        ];

        for &(x, ys, z) in test_cases {
            let simd = bn.compute_4x(x, ys, z);
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
    fn test_precompute_column_matches_scalar() {
        let bn = BlendedNoise::new(&mut make_source(42), 1.0, 1.0, 80.0, 160.0, 8.0);

        // 49 Y values like the actual overworld (cell_min_y=-8, corners_y=49, cell_height=8)
        let block_ys: Vec<i32> = (0..49).map(|cy| (cy - 8) * 8).collect();

        // Compute scalar reference values first
        let scalar_results: Vec<f64> = block_ys
            .iter()
            .map(|&y| bn.compute_scalar(0, y, 0))
            .collect();

        // Precompute column and verify cache hits match
        bn.precompute_column(0, &block_ys, 0);
        for (i, &y) in block_ys.iter().enumerate() {
            let cached = bn.compute(0, y, 0);
            assert!(
                (scalar_results[i] - cached).abs() < 1e-12,
                "Column mismatch at y={y}: scalar={}, cached={cached}, diff={}",
                scalar_results[i],
                (scalar_results[i] - cached).abs(),
            );
        }

        // Invalidate and verify fallback to scalar
        BlendedNoise::invalidate_column_cache();
        let uncached = bn.compute(0, block_ys[0], 0);
        assert!(
            (scalar_results[0] - uncached).abs() < 1e-12,
            "Post-invalidate mismatch",
        );
    }

    #[test]
    fn test_blended_noise_deterministic() {
        let bn1 = BlendedNoise::new(&mut make_source(12345), 1.0, 1.0, 80.0, 160.0, 8.0);
        let bn2 = BlendedNoise::new(&mut make_source(12345), 1.0, 1.0, 80.0, 160.0, 8.0);

        let v1 = bn1.compute(0, 64, 0);
        let v2 = bn2.compute(0, 64, 0);
        assert!(
            (v1 - v2).abs() < 1e-15,
            "BlendedNoise not deterministic: {v1} vs {v2}",
        );
    }

    #[test]
    fn test_blended_noise_spatial_variation() {
        let bn = BlendedNoise::new(&mut make_source(42), 1.0, 1.0, 80.0, 160.0, 8.0);

        let values: Vec<f64> = (-5..5).map(|x| bn.compute(x * 16, 64, 0)).collect();

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
                let v = bn.compute(x * 16, y * 4, x * 16);
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
