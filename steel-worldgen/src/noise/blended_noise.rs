//! Float-based blended terrain noise from vanilla 26.3.

use crate::noise::ImprovedNoise;
use crate::random::RandomSource;
use std::simd::cmp::SimdPartialEq;
use std::simd::num::SimdFloat;
use std::simd::{Select, Simd};
use steel_math::clamped_lerp;

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
                * layer.noise.smeared_noise(
                    x * layer.frequency,
                    y * layer.frequency,
                    z * layer.frequency,
                    layer.fudge_y_scale,
                );
        }
        value
    }

    #[inline]
    fn sample_y_simd<const N: usize>(&self, x: f64, ys: Simd<f64, N>, z: f64) -> Simd<f32, N> {
        let mut value = Simd::splat(0.0_f32);
        for layer in &self.layers {
            value += Simd::splat(layer.amplitude)
                * layer.noise.smeared_noise_y_simd(
                    x * layer.frequency,
                    ys * Simd::splat(layer.frequency),
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
        clamped_lerp(minimum, maximum, alpha)
    }

    #[inline]
    fn compute_y_simd<const N: usize>(
        &self,
        block_x: f64,
        block_ys: Simd<f64, N>,
        block_z: f64,
    ) -> Simd<f32, N> {
        let limit_x = block_x * self.xz_multiplier;
        let limit_ys = block_ys * Simd::splat(self.y_multiplier);
        let limit_z = block_z * self.xz_multiplier;
        let main = self.main_noise.sample_y_simd(
            block_x * self.main_xz_scale,
            block_ys * Simd::splat(self.main_y_scale),
            block_z * self.main_xz_scale,
        );
        let alpha = (main + Simd::splat(0.5_f32))
            .simd_max(Simd::splat(0.0))
            .simd_min(Simd::splat(1.0));
        if alpha.simd_eq(Simd::splat(0.0)).all() {
            return self
                .min_limit_noise
                .sample_y_simd(limit_x, limit_ys, limit_z);
        }
        if alpha.simd_eq(Simd::splat(1.0)).all() {
            return self
                .max_limit_noise
                .sample_y_simd(limit_x, limit_ys, limit_z);
        }
        let minimum = self
            .min_limit_noise
            .sample_y_simd(limit_x, limit_ys, limit_z);
        let maximum = self
            .max_limit_noise
            .sample_y_simd(limit_x, limit_ys, limit_z);
        let interpolated = minimum + alpha * (maximum - minimum);
        let result = alpha
            .simd_eq(Simd::splat(0.0))
            .select(minimum, interpolated);
        alpha.simd_eq(Simd::splat(1.0)).select(maximum, result)
    }

    /// Samples one X/Z column into the supplied float buffer.
    pub fn compute_column(&self, block_x: i32, block_ys: &[i32], block_z: i32, out: &mut [f32]) {
        let len = block_ys.len().min(out.len());
        let mut index = 0;
        // Four-lane batches are faster on baseline targets; retain eight for AVX-512.
        #[cfg(target_feature = "avx512f")]
        while index + 8 <= len {
            let ys: Simd<f64, 8> = Simd::from_array(std::array::from_fn(|lane| {
                f64::from(block_ys[index + lane])
            }));
            out[index..index + 8].copy_from_slice(
                &self
                    .compute_y_simd(f64::from(block_x), ys, f64::from(block_z))
                    .to_array(),
            );
            index += 8;
        }
        while index + 4 <= len {
            let ys: Simd<f64, 4> = Simd::from_array(std::array::from_fn(|lane| {
                f64::from(block_ys[index + lane])
            }));
            out[index..index + 4].copy_from_slice(
                &self
                    .compute_y_simd(f64::from(block_x), ys, f64::from(block_z))
                    .to_array(),
            );
            index += 4;
        }
        if index + 2 <= len {
            let ys: Simd<f64, 2> = Simd::from_array(std::array::from_fn(|lane| {
                f64::from(block_ys[index + lane])
            }));
            out[index..index + 2].copy_from_slice(
                &self
                    .compute_y_simd(f64::from(block_x), ys, f64::from(block_z))
                    .to_array(),
            );
            index += 2;
        }
        for (&block_y, value) in block_ys[index..len].iter().zip(&mut out[index..len]) {
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

    #[test]
    fn compute_column_matches_scalar_lanes() {
        let mut random = RandomSource::Legacy(LegacyRandom::from_seed(0));
        let noise = BlendedNoise::new(&mut random, 0.25, 0.125, 80.0, 160.0, 8.0);
        let ys = [
            -64, -56, -48, -40, -32, -24, -16, -8, 0, 8, 16, 24, 32, 40, 48, 56, 64,
        ];
        let mut column = [0.0_f32; 17];
        // Exercise every 8/4/2/scalar tail combination and short output buffers.
        for len in 0..=ys.len() {
            noise.compute_column(20_000_068, &ys, -19_999_796, &mut column[..len]);
            for (&y, &value) in ys.iter().zip(&column[..len]) {
                assert_eq!(
                    value.to_bits(),
                    noise
                        .compute(20_000_068.0, f64::from(y), -19_999_796.0)
                        .to_bits(),
                    "length={len}, Y={y}"
                );
            }
        }
    }

    #[test]
    fn simd_endpoint_shortcuts_preserve_each_lane() {
        use std::array;
        use std::simd::{Simd, num::SimdFloat};

        let mut saw_minimum = false;
        let mut saw_maximum = false;
        let mut saw_mixed = false;
        for seed in [0, 42, 13579] {
            let mut random = RandomSource::Legacy(LegacyRandom::from_seed(seed));
            let noise = BlendedNoise::new(&mut random, 0.25, 0.125, 80.0, 160.0, 8.0);
            for x in [-20_000_068.0, -128.0, 0.0, 128.0, 20_000_068.0] {
                for z in [-19_999_796.0, -256.0, 0.0, 256.0, 19_999_796.0] {
                    for base_y in (-64..256).step_by(64) {
                        let ys = Simd::from_array(array::from_fn::<_, 8, _>(|lane| {
                            f64::from(base_y) + lane as f64 * 8.0
                        }));
                        let main = noise.main_noise.sample_y_simd(
                            x * noise.main_xz_scale,
                            ys * Simd::splat(noise.main_y_scale),
                            z * noise.main_xz_scale,
                        );
                        let all_minimum = main.reduce_max() <= -0.5;
                        let all_maximum = main.reduce_min() >= 0.5;
                        saw_minimum |= all_minimum;
                        saw_maximum |= all_maximum;
                        saw_mixed |= !all_minimum && !all_maximum;
                        let actual = noise.compute_y_simd(x, ys, z);
                        for (lane, y) in ys.to_array().into_iter().enumerate() {
                            assert_eq!(
                                actual[lane].to_bits(),
                                noise.compute(x, y, z).to_bits(),
                                "seed {seed}, ({x}, {y}, {z})"
                            );
                        }
                    }
                }
            }
        }
        assert!(saw_minimum && saw_maximum && saw_mixed);
    }
}
