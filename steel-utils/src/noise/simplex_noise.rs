//! Simplex noise implementation matching vanilla Minecraft's `SimplexNoise.java`.
//!
//! Used by the End islands density function for terrain generation in The End dimension.
//! Supports 2D and 3D sampling with the same gradient vectors as Perlin noise.

use crate::math::floor;
use crate::noise::GRADIENT;
use crate::random::Random;

#[allow(clippy::unreadable_literal)]
const SQRT_3: f64 = 1.7320508075688772;
/// Skewing factor for 2D simplex: `0.5 * (sqrt(3) - 1)`
const F2: f64 = 0.5 * (SQRT_3 - 1.0);
/// Unskewing factor for 2D simplex: `(3 - sqrt(3)) / 6`
const G2: f64 = (3.0 - SQRT_3) / 6.0;

/// Simplex noise generator matching vanilla's `SimplexNoise.java`.
///
/// Unlike `ImprovedNoise` which uses 256-byte permutation tables, this uses a
/// 512-entry `i32` permutation table (first 256 entries shuffled, mirrored to second half).
#[derive(Debug, Clone)]
pub struct SimplexNoise {
    p: [i32; 512],
    /// X offset for the noise coordinates.
    pub xo: f64,
    /// Y offset for the noise coordinates.
    pub yo: f64,
    /// Z offset for the noise coordinates.
    pub zo: f64,
}

impl SimplexNoise {
    /// Create a new simplex noise generator from a random source.
    ///
    /// Matches vanilla's `SimplexNoise(RandomSource)` constructor:
    /// consumes 3 doubles for offsets, then shuffles a 256-entry permutation table.
    pub fn new<R: Random>(random: &mut R) -> Self {
        let xo = random.next_f64() * 256.0;
        let yo = random.next_f64() * 256.0;
        let zo = random.next_f64() * 256.0;

        let mut p = [0i32; 512];

        // Initialize identity permutation
        for (i, val) in p.iter_mut().enumerate().take(256) {
            *val = i as i32;
        }

        // Fisher-Yates shuffle matching vanilla's loop
        for i in 0..256 {
            let offset = random.next_i32_bounded((256 - i) as i32) as usize;
            p.swap(i, offset + i);
        }

        // Mirror first 256 entries to second half (matching vanilla)
        for i in 0..256 {
            p[i + 256] = p[i];
        }

        Self { p, xo, yo, zo }
    }

    #[inline]
    const fn p(&self, x: i32) -> i32 {
        self.p[(x & 0xFF) as usize]
    }

    /// Dot product of gradient vector and offset vector.
    #[inline]
    fn dot(g: &[i32; 3], x: f64, y: f64, z: f64) -> f64 {
        f64::from(g[0]) * x + f64::from(g[1]) * y + f64::from(g[2]) * z
    }

    /// Compute corner noise contribution for a simplex vertex.
    #[inline]
    fn corner_noise_3d(index: usize, x: f64, y: f64, z: f64, base: f64) -> f64 {
        let t0 = base - x * x - y * y - z * z;
        if t0 < 0.0 {
            0.0
        } else {
            let t0 = t0 * t0;
            t0 * t0 * Self::dot(&GRADIENT[index], x, y, z)
        }
    }

    /// Sample 2D simplex noise at the given coordinates.
    ///
    /// Returns a value typically in the range `[-1, 1]` (scaled by 70).
    #[must_use]
    pub fn get_value_2d(&self, xin: f64, yin: f64) -> f64 {
        let s = (xin + yin) * F2;
        let i = floor(xin + s);
        let j = floor(yin + s);
        let t = f64::from(i + j) * G2;
        let x0 = xin - (f64::from(i) - t);
        let y0 = yin - (f64::from(j) - t);

        // Determine which simplex triangle we're in
        let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };

        let x1 = x0 - f64::from(i1) + G2;
        let y1 = y0 - f64::from(j1) + G2;
        let x2 = x0 - 1.0 + 2.0 * G2;
        let y2 = y0 - 1.0 + 2.0 * G2;

        let ii = i & 0xFF;
        let jj = j & 0xFF;
        let gi0 = (self.p(ii + self.p(jj)) % 12) as usize;
        let gi1 = (self.p(ii + i1 + self.p(jj + j1)) % 12) as usize;
        let gi2 = (self.p(ii + 1 + self.p(jj + 1)) % 12) as usize;

        let n0 = Self::corner_noise_3d(gi0, x0, y0, 0.0, 0.5);
        let n1 = Self::corner_noise_3d(gi1, x1, y1, 0.0, 0.5);
        let n2 = Self::corner_noise_3d(gi2, x2, y2, 0.0, 0.5);

        70.0 * (n0 + n1 + n2)
    }

    /// Skewing factor for 3D simplex: `1/3`
    const F3: f64 = 1.0 / 3.0;
    /// Unskewing factor for 3D simplex: `1/6`
    const G3: f64 = 1.0 / 6.0;

    /// Sample 3D simplex noise at the given coordinates.
    ///
    /// Returns a value typically in the range `[-1, 1]` (scaled by 32).
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn get_value_3d(&self, xin: f64, yin: f64, zin: f64) -> f64 {
        let s = (xin + yin + zin) * Self::F3;
        let i = floor(xin + s);
        let j = floor(yin + s);
        let k = floor(zin + s);
        let t = f64::from(i + j + k) * Self::G3;
        let x0 = xin - (f64::from(i) - t);
        let y0 = yin - (f64::from(j) - t);
        let z0 = zin - (f64::from(k) - t);

        // Determine which simplex tetrahedron we're in
        let (i1, j1, k1, i2, j2, k2) = if x0 >= y0 {
            if y0 >= z0 {
                (1, 0, 0, 1, 1, 0)
            } else if x0 >= z0 {
                (1, 0, 0, 1, 0, 1)
            } else {
                (0, 0, 1, 1, 0, 1)
            }
        } else if y0 < z0 {
            (0, 0, 1, 0, 1, 1)
        } else if x0 < z0 {
            (0, 1, 0, 0, 1, 1)
        } else {
            (0, 1, 0, 1, 1, 0)
        };

        let x1 = x0 - f64::from(i1) + Self::G3;
        let y1 = y0 - f64::from(j1) + Self::G3;
        let z1 = z0 - f64::from(k1) + Self::G3;
        let x2 = x0 - f64::from(i2) + Self::F3;
        let y2 = y0 - f64::from(j2) + Self::F3;
        let z2 = z0 - f64::from(k2) + Self::F3;
        let x3 = x0 - 1.0 + 0.5;
        let y3 = y0 - 1.0 + 0.5;
        let z3 = z0 - 1.0 + 0.5;

        let ii = i & 0xFF;
        let jj = j & 0xFF;
        let kk = k & 0xFF;
        let gi0 = (self.p(ii + self.p(jj + self.p(kk))) % 12) as usize;
        let gi1 = (self.p(ii + i1 + self.p(jj + j1 + self.p(kk + k1))) % 12) as usize;
        let gi2 = (self.p(ii + i2 + self.p(jj + j2 + self.p(kk + k2))) % 12) as usize;
        let gi3 = (self.p(ii + 1 + self.p(jj + 1 + self.p(kk + 1))) % 12) as usize;

        let n0 = Self::corner_noise_3d(gi0, x0, y0, z0, 0.6);
        let n1 = Self::corner_noise_3d(gi1, x1, y1, z1, 0.6);
        let n2 = Self::corner_noise_3d(gi2, x2, y2, z2, 0.6);
        let n3 = Self::corner_noise_3d(gi3, x3, y3, z3, 0.6);

        32.0 * (n0 + n1 + n2 + n3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::legacy_random::LegacyRandom;

    #[test]
    fn test_simplex_noise_deterministic() {
        let mut rng = LegacyRandom::from_seed(42);
        let noise1 = SimplexNoise::new(&mut rng);

        let mut rng = LegacyRandom::from_seed(42);
        let noise2 = SimplexNoise::new(&mut rng);

        for i in 0..10 {
            let x = f64::from(i) * 13.7;
            let z = f64::from(i) * 7.3;
            #[allow(clippy::float_cmp)]
            // Determinism test: identical inputs must produce identical outputs
            {
                assert_eq!(noise1.get_value_2d(x, z), noise2.get_value_2d(x, z));
            }
        }
    }

    #[test]
    fn test_simplex_2d_spatial_variation() {
        let mut rng = LegacyRandom::from_seed(0);
        let noise = SimplexNoise::new(&mut rng);

        let values: Vec<f64> = (0..20)
            .map(|i| noise.get_value_2d(f64::from(i) * 50.0, f64::from(i) * 30.0))
            .collect();

        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(max - min > 0.01, "2D simplex should have spatial variation");
    }

    /// Verify the end-islands noise initialization: seed 0, consumeCount(17292).
    #[test]
    fn test_end_islands_noise_init() {
        let mut rng = LegacyRandom::from_seed(0);
        rng.consume_count(17292);
        let noise = SimplexNoise::new(&mut rng);

        // Verify the noise produces a finite, non-zero value at a known coordinate
        let v = noise.get_value_2d(10.0, 10.0);
        assert!(v.is_finite(), "Noise should produce a finite value");
        assert!(
            v.abs() > 1e-10,
            "Noise at (10, 10) should be non-zero, got {v}"
        );
    }
}
