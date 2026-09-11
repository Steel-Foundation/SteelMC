//! Float-based blended terrain noise from vanilla 26.3.

use crate::noise::ImprovedNoise;
use crate::random::RandomSource;

const BASE_SCALE: f64 = 684.412;

#[derive(Debug, Clone)]
struct SmearedLayer {
    noise: ImprovedNoise,
    frequency: f64,
    amplitude: f32,
    fudge_y_scale: f64,
}

#[derive(Debug, Clone)]
struct SmearedStack {
    layers: Box<[SmearedLayer]>,
}

impl SmearedStack {
    fn create(
        random: &mut RandomSource,
        first_octave: i32,
        smear_y_scale: f64,
        mut value_factor: f64,
    ) -> Self {
        let octaves = (-first_octave + 1) as usize;
        value_factor /= 2.0_f64.powi(octaves as i32) - 1.0;
        let mut frequency = 1.0;
        let mut layers = Vec::with_capacity(octaves);

        // `BlendedNoise.createFbm` initializes the highest octave first.
        for _ in (0..octaves).rev() {
            layers.push(SmearedLayer {
                noise: ImprovedNoise::new(random),
                frequency,
                amplitude: value_factor as f32,
                fudge_y_scale: smear_y_scale * frequency,
            });
            frequency *= 0.5;
            value_factor *= 2.0;
        }

        Self {
            layers: layers.into_boxed_slice(),
        }
    }

    #[inline]
    fn sample(&self, x: f64, y: f64, z: f64) -> f32 {
        let mut value = 0.0_f32;
        for layer in &self.layers {
            value += layer.amplitude
                * layer.noise.smeared_noise_f32(
                    x * layer.frequency,
                    y * layer.frequency,
                    z * layer.frequency,
                    layer.fudge_y_scale,
                );
        }
        value
    }
}

/// Runtime counterpart of vanilla's 26.3 `BlendedNoise` sampler.
///
/// The old terrain sampler used double octave sums. Vanilla now builds three
/// `NoiseStack`s and carries float values through each layer and the final lerp.
#[derive(Debug, Clone)]
pub struct BlendedNoise {
    min_limit_noise: SmearedStack,
    max_limit_noise: SmearedStack,
    main_noise: SmearedStack,
    xz_multiplier: f64,
    y_multiplier: f64,
    main_xz_scale: f64,
    main_y_scale: f64,
}

impl BlendedNoise {
    /// Creates the three seeded `NoiseStack`s used for terrain generation.
    #[must_use]
    pub fn new(
        random: &mut RandomSource,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> Self {
        let xz_multiplier = BASE_SCALE * xz_scale;
        let y_multiplier = BASE_SCALE * y_scale;
        let limit_smear_scale_y = y_multiplier * smear_scale_multiplier;
        let main_smear_scale_y = limit_smear_scale_y / y_factor;

        Self {
            min_limit_noise: SmearedStack::create(
                random,
                -15,
                limit_smear_scale_y,
                f64::from(0.999_984_74_f32),
            ),
            max_limit_noise: SmearedStack::create(
                random,
                -15,
                limit_smear_scale_y,
                f64::from(0.999_984_74_f32),
            ),
            main_noise: SmearedStack::create(random, -7, main_smear_scale_y, 12.75),
            xz_multiplier,
            y_multiplier,
            main_xz_scale: xz_multiplier / xz_factor,
            main_y_scale: y_multiplier / y_factor,
        }
    }

    #[inline]
    /// Samples the blended terrain density at a block coordinate.
    #[must_use]
    #[expect(
        clippy::float_cmp,
        reason = "Vanilla selects exact lerp endpoints before interpolation"
    )]
    pub fn compute(&self, block_x: f64, block_y: f64, block_z: f64) -> f32 {
        let limit_x = block_x * self.xz_multiplier;
        let limit_y = block_y * self.y_multiplier;
        let limit_z = block_z * self.xz_multiplier;
        let main = self.main_noise.sample(
            block_x * self.main_xz_scale,
            block_y * self.main_y_scale,
            block_z * self.main_xz_scale,
        );
        let alpha = (main + 0.5_f32).clamp(0.0, 1.0);
        if alpha == 0.0 {
            return self.min_limit_noise.sample(limit_x, limit_y, limit_z);
        }
        if alpha == 1.0 {
            return self.max_limit_noise.sample(limit_x, limit_y, limit_z);
        }
        let minimum = self.min_limit_noise.sample(limit_x, limit_y, limit_z);
        let maximum = self.max_limit_noise.sample(limit_x, limit_y, limit_z);
        minimum + alpha * (maximum - minimum)
    }

    /// Samples one X/Z column into the supplied float buffer.
    pub fn compute_column(&self, block_x: i32, block_ys: &[i32], block_z: i32, out: &mut [f32]) {
        for (&block_y, value) in block_ys.iter().zip(out) {
            *value = self.compute(f64::from(block_x), f64::from(block_y), f64::from(block_z));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BlendedNoise;
    use crate::random::{RandomSource, legacy_random::LegacyRandom};

    #[test]
    fn vanilla_lerp_endpoint_and_main_coordinate_rounding() {
        let mut random = RandomSource::Legacy(LegacyRandom::from_seed(0));
        let noise = BlendedNoise::new(&mut random, 0.25, 0.125, 80.0, 160.0, 8.0);

        for (x, y, z, expected) in [
            (0.0, -56.0, -10_000.0, 0.410_753_88_f32),
            (20_000_068.0, 296.0, -19_999_796.0, 0.013_388_243_f32),
        ] {
            assert_eq!(noise.compute(x, y, z).to_bits(), expected.to_bits());
        }
    }
}
