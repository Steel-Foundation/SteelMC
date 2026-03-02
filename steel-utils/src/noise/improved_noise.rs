//! Improved Perlin noise implementation matching vanilla Minecraft's ImprovedNoise.java
//!
//! This is the base noise generator used by `PerlinNoise` for octave-based noise.

use crate::math::{floor, lerp3, smoothstep};
use crate::noise::GRADIENT;
use crate::random::Random;

/// Improved Perlin noise generator.
///
/// This implements the improved Perlin noise algorithm as used in Minecraft.
/// Each instance has a permutation table and offset values initialized from
/// a random source.
///
// TODO: Implement `noise_with_derivative()` returning `(value, dx, dy, dz)` for
// full BlendedNoise support and density function derivatives.
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
        #[allow(clippy::needless_range_loop)]
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
    #[allow(clippy::many_single_char_names, clippy::similar_names)]
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
        #[allow(clippy::if_not_else)]
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
    #[allow(clippy::too_many_arguments)]
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
}

/// Calculate the dot product of a gradient vector and the position vector.
#[inline]
fn grad_dot(hash: usize, x: f64, y: f64, z: f64) -> f64 {
    let g = &GRADIENT[hash & 15];
    f64::from(g[0]) * x + f64::from(g[1]) * y + f64::from(g[2]) * z
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::xoroshiro::Xoroshiro;

    #[test]
    fn test_improved_noise_deterministic() {
        let mut rng1 = Xoroshiro::from_seed(12345);
        let mut rng2 = Xoroshiro::from_seed(12345);

        let noise1 = ImprovedNoise::new(&mut rng1);
        let noise2 = ImprovedNoise::new(&mut rng2);

        // Same seed should produce same noise
        #[allow(clippy::float_cmp)]
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
        #[allow(clippy::float_cmp)]
        let all_same = v1 == v2 && v2 == v3 && v3 == v4;
        assert!(!all_same, "All noise values are the same - unexpected");
    }
}
