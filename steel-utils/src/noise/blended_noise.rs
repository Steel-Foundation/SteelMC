//! `BlendedNoise` implementation matching vanilla Minecraft's `BlendedNoise.java`
//!
//! Combines three `PerlinNoise` instances (min limit, max limit, main) for terrain generation.
//! The main noise determines the blend factor between the min and max limit noises.

use crate::math::clamped_lerp;
use crate::noise::PerlinNoise;
use crate::noise::perlin_noise::wrap;
use crate::random::RandomSource;

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
    /// This is the core terrain density computation. It:
    /// 1. Samples the main noise (8 octaves) to get a blend factor
    /// 2. Conditionally samples min/max limit noises (16 octaves each)
    /// 3. Interpolates between min and max based on the blend factor
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn compute(&self, block_x: i32, block_y: i32, block_z: i32) -> f64 {
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
