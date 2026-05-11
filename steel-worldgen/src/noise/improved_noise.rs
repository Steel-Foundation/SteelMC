//! Improved Perlin noise implementation matching vanilla Minecraft's ImprovedNoise.java
//!
//! This is the base noise generator used by `PerlinNoise` for octave-based noise.

use std::simd::cmp::SimdPartialOrd;
use std::simd::f64x4;
use std::simd::{Select, StdFloat};

use crate::math::{floor, lerp2, lerp3, smoothstep, smoothstep_derivative};
use crate::noise::GRADIENT;
use crate::random::Random;

/// Improved Perlin noise generator.
///
/// This implements the improved Perlin noise algorithm as used in Minecraft.
/// Each instance has a permutation table and offset values initialized from
/// a random source.
#[derive(Debug, Clone)]
pub struct ImprovedNoise {
    /// Permutation table (256 bytes)
    p: [u8; 256],
    /// X offset for the noise coordinates
    pub xo: f64,
    /// Y offset for the noise coordinates
    pub yo: f64,
    /// Z offset for the noise coordinates
    pub zo: f64,
}

impl ImprovedNoise {
    /// Creates a new `ImprovedNoise` from a random source.
    ///
    /// Initializes the permutation table using Fisher-Yates shuffle
    /// and sets random offsets.
    pub fn new<R: Random>(random: &mut R) -> Self {
        let xo = random.next_f64() * 256.0;
        let yo = random.next_f64() * 256.0;
        let zo = random.next_f64() * 256.0;

        let mut p = [0u8; 256];
        #[expect(
            clippy::needless_range_loop,
            reason = "index is used as the initial permutation value"
        )]
        for i in 0..256 {
            p[i] = i as u8;
        }

        // Fisher-Yates shuffle matching vanilla's implementation
        for i in 0..256 {
            let offset = random.next_i32_bounded((256 - i) as i32) as usize;
            p.swap(i, i + offset);
        }

        Self { p, xo, yo, zo }
    }

    /// Sample noise at the given coordinates.
    ///
    /// This is the standard 3D Perlin noise sampling without Y scaling.
    #[inline]
    #[must_use]
    pub fn noise(&self, x: f64, y: f64, z: f64) -> f64 {
        self.noise_with_y_scale(x, y, z, 0.0, 0.0)
    }

    /// Sample noise at the given coordinates, accumulating partial derivatives.
    ///
    /// Returns the noise value and adds the partial derivatives (dx, dy, dz)
    /// into `derivative_out`. Used by `BlendedNoise` for terrain generation.
    #[must_use]
    pub fn noise_with_derivative(
        &self,
        x: f64,
        y: f64,
        z: f64,
        derivative_out: &mut [f64; 3],
    ) -> f64 {
        let x = x + self.xo;
        let y = y + self.yo;
        let z = z + self.zo;

        let xf = floor(x);
        let yf = floor(y);
        let zf = floor(z);

        let xr = x - f64::from(xf);
        let yr = y - f64::from(yf);
        let zr = z - f64::from(zf);

        self.sample_with_derivative(xf, yf, zf, xr, yr, zr, derivative_out)
    }

    /// Sample noise with Y scale and fudge parameters.
    ///
    /// The `y_scale` and `y_fudge` parameters are used for terrain generation
    /// where vertical noise needs special handling.
    ///
    /// # Arguments
    /// * `x`, `y`, `z` - The coordinates to sample
    /// * `y_scale` - Y scaling factor (0.0 to disable)
    /// * `y_fudge` - Y fudge factor for floor snapping
    #[must_use]
    #[expect(
        clippy::similar_names,
        reason = "yr_fudge and y_fudge match vanilla naming"
    )]
    pub fn noise_with_y_scale(&self, x: f64, y: f64, z: f64, y_scale: f64, y_fudge: f64) -> f64 {
        let x = x + self.xo;
        let y = y + self.yo;
        let z = z + self.zo;

        let xf = floor(x);
        let yf = floor(y);
        let zf = floor(z);

        let xr = x - f64::from(xf);
        let yr = y - f64::from(yf);
        let zr = z - f64::from(zf);

        // Calculate Y fudge for terrain generation
        #[expect(
            clippy::if_not_else,
            reason = "matches vanilla's conditional structure"
        )]
        let yr_fudge = if y_scale != 0.0 {
            let fudge_limit = if y_fudge >= 0.0 && y_fudge < yr {
                y_fudge
            } else {
                yr
            };
            // SHIFT_UP_EPSILON = 1.0E-7F in Java (float literal promoted to double)
            (fudge_limit / y_scale + f64::from(1.0e-7_f32)).floor() * y_scale
        } else {
            0.0
        };

        self.sample_and_lerp(xf, yf, zf, xr, yr - yr_fudge, zr, yr)
    }

    /// Look up the permutation value at index x.
    #[inline]
    const fn p(&self, x: i32) -> usize {
        self.p[(x & 255) as usize] as usize
    }

    /// Sample noise at grid point and interpolate.
    #[expect(clippy::too_many_arguments, reason = "matches vanilla signature")]
    fn sample_and_lerp(
        &self,
        x: i32,
        y: i32,
        z: i32,
        xr: f64,
        yr: f64,
        zr: f64,
        yr_original: f64,
    ) -> f64 {
        // Get permutation indices for the 8 corners
        let x0 = self.p(x);
        let x1 = self.p(x + 1);
        let xy00 = self.p(x0 as i32 + y);
        let xy01 = self.p(x0 as i32 + y + 1);
        let xy10 = self.p(x1 as i32 + y);
        let xy11 = self.p(x1 as i32 + y + 1);

        // Calculate gradient dot products at each corner
        let d000 = grad_dot(self.p(xy00 as i32 + z), xr, yr, zr);
        let d100 = grad_dot(self.p(xy10 as i32 + z), xr - 1.0, yr, zr);
        let d010 = grad_dot(self.p(xy01 as i32 + z), xr, yr - 1.0, zr);
        let d110 = grad_dot(self.p(xy11 as i32 + z), xr - 1.0, yr - 1.0, zr);
        let d001 = grad_dot(self.p(xy00 as i32 + z + 1), xr, yr, zr - 1.0);
        let d101 = grad_dot(self.p(xy10 as i32 + z + 1), xr - 1.0, yr, zr - 1.0);
        let d011 = grad_dot(self.p(xy01 as i32 + z + 1), xr, yr - 1.0, zr - 1.0);
        let d111 = grad_dot(self.p(xy11 as i32 + z + 1), xr - 1.0, yr - 1.0, zr - 1.0);

        // Apply smoothstep interpolation
        let x_alpha = smoothstep(xr);
        let y_alpha = smoothstep(yr_original);
        let z_alpha = smoothstep(zr);

        lerp3(
            x_alpha, y_alpha, z_alpha, d000, d100, d010, d110, d001, d101, d011, d111,
        )
    }

    // -----------------------------------------------------------------------
    // SIMD: process 4 Y values sharing the same (x, z)
    // -----------------------------------------------------------------------

    /// Sample noise for 4 points that share the same x/z but differ in y.
    ///
    /// This is the SIMD counterpart of [`noise_with_y_scale`]. The x and z
    /// coordinate work (offset, floor, permutation) is done once and reused
    /// across all 4 lanes, while the y-dependent math is vectorized.
    #[must_use]
    pub fn noise_with_y_scale_4x(
        &self,
        x: f64,
        ys: f64x4,
        z: f64,
        y_scale: f64,
        y_fudges: f64x4,
    ) -> f64x4 {
        // Shared x/z offset and floor
        let x = x + self.xo;
        let z = z + self.zo;
        let xf = floor(x);
        let zf = floor(z);
        let xr = x - f64::from(xf);
        let zr = z - f64::from(zf);

        // Per-lane y offset and floor
        let ys = ys + f64x4::splat(self.yo);
        let ys_floor = ys.floor();
        let yrs = ys - ys_floor;

        // Y fudge (per-lane)
        let yr_fudge = if y_scale == 0.0 {
            f64x4::splat(0.0)
        } else {
            let y_scale_v = f64x4::splat(y_scale);
            let zero = f64x4::splat(0.0);
            let mask = y_fudges.simd_ge(zero) & y_fudges.simd_lt(yrs);
            let fudge_limits = mask.select(y_fudges, yrs);
            let epsilon = f64x4::splat(f64::from(1.0e-7_f32));
            ((fudge_limits / y_scale_v) + epsilon).floor() * y_scale_v
        };

        let yrs_adjusted = yrs - yr_fudge;

        self.sample_and_lerp_4x(xf, zf, xr, zr, ys_floor, yrs_adjusted, yrs)
    }

    /// Vectorized sample-and-lerp for 4 Y values sharing x/z grid position.
    ///
    /// `ys_floor` contains the floored y coordinates (as f64 for extraction),
    /// `yrs` are the adjusted fractional y parts, `yrs_original` are the
    /// un-fudged fractional parts (used for smoothstep).
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors scalar sample_and_lerp with 4x SIMD y-batching"
    )]
    fn sample_and_lerp_4x(
        &self,
        xf: i32,
        zf: i32,
        xr: f64,
        zr: f64,
        ys_floor: f64x4,
        yrs: f64x4,
        yrs_original: f64x4,
    ) -> f64x4 {
        // Shared x permutation lookups (2 instead of 2×4)
        let x0 = self.p(xf);
        let x1 = self.p(xf + 1);

        // Per-lane y-dependent permutation lookups
        let yf = [
            ys_floor[0] as i32,
            ys_floor[1] as i32,
            ys_floor[2] as i32,
            ys_floor[3] as i32,
        ];

        let mut h000 = [0usize; 4];
        let mut h100 = [0usize; 4];
        let mut h010 = [0usize; 4];
        let mut h110 = [0usize; 4];
        let mut h001 = [0usize; 4];
        let mut h101 = [0usize; 4];
        let mut h011 = [0usize; 4];
        let mut h111 = [0usize; 4];

        for i in 0..4 {
            let y = yf[i];
            let xy00 = self.p(x0 as i32 + y);
            let xy01 = self.p(x0 as i32 + y + 1);
            let xy10 = self.p(x1 as i32 + y);
            let xy11 = self.p(x1 as i32 + y + 1);
            h000[i] = self.p(xy00 as i32 + zf);
            h100[i] = self.p(xy10 as i32 + zf);
            h010[i] = self.p(xy01 as i32 + zf);
            h110[i] = self.p(xy11 as i32 + zf);
            h001[i] = self.p(xy00 as i32 + zf + 1);
            h101[i] = self.p(xy10 as i32 + zf + 1);
            h011[i] = self.p(xy01 as i32 + zf + 1);
            h111[i] = self.p(xy11 as i32 + zf + 1);
        }

        // Vectorized gradient dot products
        let xr_v = f64x4::splat(xr);
        let zr_v = f64x4::splat(zr);
        let xr_m1 = xr_v - f64x4::splat(1.0);
        let yr_m1 = yrs - f64x4::splat(1.0);
        let zr_m1 = zr_v - f64x4::splat(1.0);

        let d000 = grad_dot_4x(h000, xr_v, yrs, zr_v);
        let d100 = grad_dot_4x(h100, xr_m1, yrs, zr_v);
        let d010 = grad_dot_4x(h010, xr_v, yr_m1, zr_v);
        let d110 = grad_dot_4x(h110, xr_m1, yr_m1, zr_v);
        let d001 = grad_dot_4x(h001, xr_v, yrs, zr_m1);
        let d101 = grad_dot_4x(h101, xr_m1, yrs, zr_m1);
        let d011 = grad_dot_4x(h011, xr_v, yr_m1, zr_m1);
        let d111 = grad_dot_4x(h111, xr_m1, yr_m1, zr_m1);

        // Smoothstep — x and z are shared across lanes
        let x_alpha = f64x4::splat(smoothstep(xr));
        let y_alpha = smoothstep_4x(yrs_original);
        let z_alpha = f64x4::splat(smoothstep(zr));

        lerp3_4x(
            x_alpha, y_alpha, z_alpha, d000, d100, d010, d110, d001, d101, d011, d111,
        )
    }

    /// Sample noise at grid point, interpolate, and accumulate derivatives.
    #[expect(clippy::too_many_arguments, reason = "matches vanilla signature")]
    fn sample_with_derivative(
        &self,
        x: i32,
        y: i32,
        z: i32,
        xr: f64,
        yr: f64,
        zr: f64,
        derivative_out: &mut [f64; 3],
    ) -> f64 {
        let x0 = self.p(x);
        let x1 = self.p(x + 1);
        let xy00 = self.p(x0 as i32 + y);
        let xy01 = self.p(x0 as i32 + y + 1);
        let xy10 = self.p(x1 as i32 + y);
        let xy11 = self.p(x1 as i32 + y + 1);

        // Get hashes and gradient vectors for all 8 corners
        let h000 = self.p(xy00 as i32 + z);
        let h100 = self.p(xy10 as i32 + z);
        let h010 = self.p(xy01 as i32 + z);
        let h110 = self.p(xy11 as i32 + z);
        let h001 = self.p(xy00 as i32 + z + 1);
        let h101 = self.p(xy10 as i32 + z + 1);
        let h011 = self.p(xy01 as i32 + z + 1);
        let h111 = self.p(xy11 as i32 + z + 1);

        let g000 = &GRADIENT[h000 & 15];
        let g100 = &GRADIENT[h100 & 15];
        let g010 = &GRADIENT[h010 & 15];
        let g110 = &GRADIENT[h110 & 15];
        let g001 = &GRADIENT[h001 & 15];
        let g101 = &GRADIENT[h101 & 15];
        let g011 = &GRADIENT[h011 & 15];
        let g111 = &GRADIENT[h111 & 15];

        // Gradient dot products at each corner
        let d000 = grad_dot(h000, xr, yr, zr);
        let d100 = grad_dot(h100, xr - 1.0, yr, zr);
        let d010 = grad_dot(h010, xr, yr - 1.0, zr);
        let d110 = grad_dot(h110, xr - 1.0, yr - 1.0, zr);
        let d001 = grad_dot(h001, xr, yr, zr - 1.0);
        let d101 = grad_dot(h101, xr - 1.0, yr, zr - 1.0);
        let d011 = grad_dot(h011, xr, yr - 1.0, zr - 1.0);
        let d111 = grad_dot(h111, xr - 1.0, yr - 1.0, zr - 1.0);

        let x_alpha = smoothstep(xr);
        let y_alpha = smoothstep(yr);
        let z_alpha = smoothstep(zr);

        // Interpolate gradient components for direct derivative contribution
        let d1x = lerp3(
            x_alpha, y_alpha, z_alpha, g000[0], g100[0], g010[0], g110[0], g001[0], g101[0],
            g011[0], g111[0],
        );
        let d1y = lerp3(
            x_alpha, y_alpha, z_alpha, g000[1], g100[1], g010[1], g110[1], g001[1], g101[1],
            g011[1], g111[1],
        );
        let d1z = lerp3(
            x_alpha, y_alpha, z_alpha, g000[2], g100[2], g010[2], g110[2], g001[2], g101[2],
            g011[2], g111[2],
        );

        // Smoothstep correction terms via differences
        let d2x = lerp2(
            y_alpha,
            z_alpha,
            d100 - d000,
            d110 - d010,
            d101 - d001,
            d111 - d011,
        );
        let d2y = lerp2(
            z_alpha,
            x_alpha,
            d010 - d000,
            d011 - d001,
            d110 - d100,
            d111 - d101,
        );
        let d2z = lerp2(
            x_alpha,
            y_alpha,
            d001 - d000,
            d101 - d100,
            d011 - d010,
            d111 - d110,
        );

        let x_sd = smoothstep_derivative(xr);
        let y_sd = smoothstep_derivative(yr);
        let z_sd = smoothstep_derivative(zr);

        // Accumulate derivatives (vanilla uses +=)
        derivative_out[0] += d1x + x_sd * d2x;
        derivative_out[1] += d1y + y_sd * d2y;
        derivative_out[2] += d1z + z_sd * d2z;

        lerp3(
            x_alpha, y_alpha, z_alpha, d000, d100, d010, d110, d001, d101, d011, d111,
        )
    }
}

// ---------------------------------------------------------------------------
// SIMD helpers (4-wide f64)
// ---------------------------------------------------------------------------

/// Smoothstep for 4 lanes: 6x^5 - 15x^4 + 10x^3
#[inline]
fn smoothstep_4x(x: f64x4) -> f64x4 {
    x * x * x * (x * (x * f64x4::splat(6.0) - f64x4::splat(15.0)) + f64x4::splat(10.0))
}

/// Trilinear interpolation for 4 lanes.
#[inline]
fn lerp_4x(alpha: f64x4, a: f64x4, b: f64x4) -> f64x4 {
    a + alpha * (b - a)
}

#[inline]
fn lerp2_4x(a1: f64x4, a2: f64x4, x00: f64x4, x10: f64x4, x01: f64x4, x11: f64x4) -> f64x4 {
    lerp_4x(a2, lerp_4x(a1, x00, x10), lerp_4x(a1, x01, x11))
}

#[inline]
#[expect(clippy::too_many_arguments, reason = "mirrors lerp3 with SIMD vectors")]
fn lerp3_4x(
    a1: f64x4,
    a2: f64x4,
    a3: f64x4,
    x000: f64x4,
    x100: f64x4,
    x010: f64x4,
    x110: f64x4,
    x001: f64x4,
    x101: f64x4,
    x011: f64x4,
    x111: f64x4,
) -> f64x4 {
    lerp_4x(
        a3,
        lerp2_4x(a1, a2, x000, x100, x010, x110),
        lerp2_4x(a1, a2, x001, x101, x011, x111),
    )
}

/// Gather gradient components for 4 hashes into separate x/y/z SIMD vectors,
/// then compute the dot product with the given position vectors.
#[inline]
fn grad_dot_4x(hashes: [usize; 4], x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
    let mut gx = [0.0f64; 4];
    let mut gy = [0.0f64; 4];
    let mut gz = [0.0f64; 4];
    for i in 0..4 {
        let g = &GRADIENT[hashes[i] & 15];
        gx[i] = g[0];
        gy[i] = g[1];
        gz[i] = g[2];
    }
    f64x4::from_array(gx) * x + f64x4::from_array(gy) * y + f64x4::from_array(gz) * z
}

/// Calculate the dot product of a gradient vector and the position vector.
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
fn grad_dot(hash: usize, x: f64, y: f64, z: f64) -> f64 {
    let g = &GRADIENT[hash & 15];
    g[0] * x + g[1] * y + g[2] * z
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::xoroshiro::Xoroshiro;

    #[test]
    fn test_noise_with_y_scale_4x_matches_scalar() {
        let mut rng = Xoroshiro::from_seed(42);
        let noise = ImprovedNoise::new(&mut rng);

        // Test various coordinate combinations
        let test_x_zs: &[(f64, f64)] = &[
            (0.0, 0.0),
            (1.5, 3.7),
            (-5.2, 100.3),
            (0.001, -0.001),
            (1000.0, -500.0),
        ];
        let test_ys: &[[f64; 4]] = &[
            [0.0, 1.0, 2.0, 3.0],
            [64.0, 64.5, 65.0, 65.5],
            [-5.0, -2.5, 0.0, 2.5],
            [0.25, 0.5, 0.75, 1.0],
            [-100.0, -50.0, 50.0, 100.0],
        ];
        let y_scales = [0.0, 1.0, 8.0];

        for &(x, z) in test_x_zs {
            for ys in test_ys {
                for &y_scale in &y_scales {
                    let y_fudges: [f64; 4] = if y_scale == 0.0 {
                        [0.0; 4]
                    } else {
                        *ys // use ys as fudge values (matching BlendedNoise usage)
                    };

                    let simd_result = noise.noise_with_y_scale_4x(
                        x,
                        f64x4::from_array(*ys),
                        z,
                        y_scale,
                        f64x4::from_array(y_fudges),
                    );

                    for i in 0..4 {
                        let scalar = noise.noise_with_y_scale(x, ys[i], z, y_scale, y_fudges[i]);
                        let simd_val = simd_result[i];
                        assert!(
                            (scalar - simd_val).abs() < 1e-14,
                            "Mismatch at x={x}, y={}, z={z}, y_scale={y_scale}: \
                             scalar={scalar}, simd={simd_val}, diff={}",
                            ys[i],
                            (scalar - simd_val).abs(),
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_improved_noise_deterministic() {
        let mut rng1 = Xoroshiro::from_seed(12345);
        let mut rng2 = Xoroshiro::from_seed(12345);

        let noise1 = ImprovedNoise::new(&mut rng1);
        let noise2 = ImprovedNoise::new(&mut rng2);

        // Same seed should produce same noise
        #[expect(
            clippy::float_cmp,
            reason = "determinism test: identical seeds must produce bit-identical offsets"
        )]
        {
            assert_eq!(noise1.xo, noise2.xo);
            assert_eq!(noise1.yo, noise2.yo);
            assert_eq!(noise1.zo, noise2.zo);
        }
        assert_eq!(noise1.p, noise2.p);

        // Same coordinates should produce same values
        let v1 = noise1.noise(100.0, 64.0, 100.0);
        let v2 = noise2.noise(100.0, 64.0, 100.0);
        assert!((v1 - v2).abs() < 1e-15);
    }

    #[test]
    fn test_improved_noise_range() {
        let mut rng = Xoroshiro::from_seed(42);
        let noise = ImprovedNoise::new(&mut rng);

        // Sample at various points and verify output is in reasonable range
        for x in -10..10 {
            for z in -10..10 {
                let v = noise.noise(f64::from(x) * 10.0, 64.0, f64::from(z) * 10.0);
                // Perlin noise should be in [-1, 1] range roughly
                assert!(
                    (-1.5..=1.5).contains(&v),
                    "Noise value {v} at ({x}, {z}) out of expected range",
                );
            }
        }
    }

    #[test]
    fn test_improved_noise_spatial_variation() {
        let mut rng = Xoroshiro::from_seed(42);
        let noise = ImprovedNoise::new(&mut rng);

        // Noise at different positions should generally be different
        let v1 = noise.noise(0.0, 0.0, 0.0);
        let v2 = noise.noise(100.0, 0.0, 0.0);
        let v3 = noise.noise(0.0, 100.0, 0.0);
        let v4 = noise.noise(0.0, 0.0, 100.0);

        // At least some should be different (statistically almost certain)
        #[expect(
            clippy::float_cmp,
            reason = "intentional exact equality check to detect degenerate constant noise"
        )]
        let all_same = v1 == v2 && v2 == v3 && v3 == v4;
        assert!(!all_same, "All noise values are the same - unexpected");
    }

    #[test]
    fn test_noise_with_derivative_matches_noise() {
        let mut rng = Xoroshiro::from_seed(42);
        let noise = ImprovedNoise::new(&mut rng);

        // noise_with_derivative should return the same value as noise()
        // (when no y_scale/y_fudge is used)
        for &(x, y, z) in &[
            (0.0, 0.0, 0.0),
            (1.5, 2.3, 3.7),
            (-5.2, 64.0, 100.3),
            (0.25, 0.25, 0.25),
        ] {
            let v1 = noise.noise(x, y, z);
            let mut deriv = [0.0; 3];
            let v2 = noise.noise_with_derivative(x, y, z, &mut deriv);
            assert!(
                (v1 - v2).abs() < 1e-12,
                "Value mismatch at ({x}, {y}, {z}): {v1} vs {v2}",
            );
        }
    }

    #[test]
    fn test_noise_with_derivative_produces_derivatives() {
        let mut rng = Xoroshiro::from_seed(42);
        let noise = ImprovedNoise::new(&mut rng);

        let mut deriv = [0.0; 3];
        let _ = noise.noise_with_derivative(1.5, 2.3, 3.7, &mut deriv);

        // At a non-grid point, at least some derivatives should be nonzero
        let any_nonzero = deriv.iter().any(|&d| d.abs() > 1e-15);
        assert!(any_nonzero, "All derivatives are zero: {deriv:?}");
    }

    #[test]
    fn test_noise_with_derivative_accumulates() {
        let mut rng = Xoroshiro::from_seed(42);
        let noise = ImprovedNoise::new(&mut rng);

        // First call
        let mut deriv = [0.0; 3];
        let _ = noise.noise_with_derivative(1.5, 2.3, 3.7, &mut deriv);
        let first = deriv;

        // Second call should accumulate (+=)
        let _ = noise.noise_with_derivative(4.1, 5.2, 6.3, &mut deriv);
        let mut deriv2 = [0.0; 3];
        let _ = noise.noise_with_derivative(4.1, 5.2, 6.3, &mut deriv2);

        for i in 0..3 {
            let expected = first[i] + deriv2[i];
            assert!(
                (deriv[i] - expected).abs() < 1e-12,
                "Derivative[{i}] not accumulated: {0} vs expected {expected}",
                deriv[i],
            );
        }
    }
}
