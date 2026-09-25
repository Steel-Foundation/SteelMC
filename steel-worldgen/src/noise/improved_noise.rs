//! Improved Perlin noise implementation matching vanilla Minecraft's ImprovedNoise.java
//!
//! This is the base noise generator used by `PerlinNoise` for octave-based noise.

use crate::random::Random;
use std::simd::Simd;
use std::simd::cmp::{SimdPartialEq, SimdPartialOrd};
use std::simd::num::{SimdFloat, SimdInt, SimdUint};
use std::simd::ptr::SimdConstPtr;
use std::simd::{Mask, Select, StdFloat};
use steel_math::{
    GRADIENT_F32, fast_floor, fast_floor_simd, grad_dot, grad_dot_simd, lerp, lerp_simd, lerp2,
    lerp2_simd, lerp3, lerp3_simd, smoothstep, smoothstep_derivative, smoothstep_simd,
};

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
    yo_floor: i32,
    yo_fraction: f64,
    zo_floor: i32,
    zo_fraction: f64,
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

        let yo_floor = fast_floor(yo);
        let yo_fraction = yo - f64::from(yo_floor);
        let zo_floor = fast_floor(zo);
        let zo_fraction = zo - f64::from(zo_floor);

        Self {
            p,
            xo,
            yo,
            zo,
            yo_floor,
            yo_fraction,
            zo_floor,
            zo_fraction,
        }
    }

    /// Samples the 26.3 float-based `PerlinNoise` implementation.
    ///
    /// Vanilla keeps coordinates and offsets as doubles, then converts the
    /// fractional coordinates and every interpolation operation to `float`.
    #[inline]
    #[must_use]
    pub fn noise(&self, x: f64, y: f64, z: f64) -> f32 {
        let x = steel_math::wrap(x) + self.xo;
        let y = steel_math::wrap(y) + self.yo;
        let z = steel_math::wrap(z) + self.zo;
        let floor_x = fast_floor(x);
        let floor_y = fast_floor(y);
        let floor_z = fast_floor(z);
        self.sample_and_lerp(
            floor_x,
            floor_y,
            floor_z,
            (x - f64::from(floor_x)) as f32,
            (y - f64::from(floor_y)) as f32,
            (z - f64::from(floor_z)) as f32,
            (y - f64::from(floor_y)) as f32,
        )
    }

    /// Samples noise at `(x, 0.0, z)`.
    #[inline]
    #[must_use]
    pub fn noise_xz(&self, x: f64, z: f64) -> f32 {
        let x = steel_math::wrap(x) + self.xo;
        let z = steel_math::wrap(z) + self.zo;
        let floor_x = fast_floor(x);
        let floor_z = fast_floor(z);
        self.sample_and_lerp(
            floor_x,
            self.yo_floor,
            floor_z,
            (x - f64::from(floor_x)) as f32,
            self.yo_fraction as f32,
            (z - f64::from(floor_z)) as f32,
            self.yo_fraction as f32,
        )
    }

    /// Samples noise at `(x, y, 0.0)`.
    #[inline]
    #[must_use]
    pub fn noise_xy(&self, x: f64, y: f64) -> f32 {
        let x = steel_math::wrap(x) + self.xo;
        let y = steel_math::wrap(y) + self.yo;
        let floor_x = fast_floor(x);
        let floor_y = fast_floor(y);
        self.sample_and_lerp(
            floor_x,
            floor_y,
            self.zo_floor,
            (x - f64::from(floor_x)) as f32,
            (y - f64::from(floor_y)) as f32,
            self.zo_fraction as f32,
            (y - f64::from(floor_y)) as f32,
        )
    }

    /// samples one X/Z column of the floatvalued Perlin noise impl
    #[inline]
    #[must_use]
    pub fn noise_y_simd<const N: usize>(&self, x: f64, ys: Simd<f64, N>, z: f64) -> Simd<f32, N> {
        let x = steel_math::wrap(x) + self.xo;
        let z = steel_math::wrap(z) + self.zo;
        let ys = steel_math::wrap_simd(ys) + Simd::splat(self.yo);
        let floor_x = fast_floor(x);
        let floor_z = fast_floor(z);
        let floor_ys = fast_floor_simd::<f64, i32, N>(ys);
        let relative_ys = (ys - floor_ys.cast()).cast();

        self.sample_and_lerp_y_simd(
            floor_x,
            floor_ys,
            floor_z,
            (x - f64::from(floor_x)) as f32,
            relative_ys,
            (z - f64::from(floor_z)) as f32,
            relative_ys,
        )
    }

    /// Samples the float-based `SmearedPerlinNoise` used by 26.3's blended
    /// terrain noise.
    #[inline]
    #[must_use]
    pub fn smeared_noise(
        &self,
        original_x: f64,
        original_y: f64,
        original_z: f64,
        fudge_y_scale: f64,
    ) -> f32 {
        let x = steel_math::wrap(original_x) + self.xo;
        let y = steel_math::wrap(original_y) + self.yo;
        let z = steel_math::wrap(original_z) + self.zo;
        let floor_x = fast_floor(x);
        let floor_y = fast_floor(y);
        let floor_z = fast_floor(z);
        let relative_y = y - f64::from(floor_y);
        let fudge_limit = if original_y >= 0.0 && original_y < relative_y {
            original_y
        } else {
            relative_y
        };
        let fudge = (fudge_limit / fudge_y_scale + f64::from(1.0e-7_f32)).floor() * fudge_y_scale;
        self.sample_and_lerp(
            floor_x,
            floor_y,
            floor_z,
            (x - f64::from(floor_x)) as f32,
            (relative_y - fudge) as f32,
            (z - f64::from(floor_z)) as f32,
            relative_y as f32,
        )
    }

    /// Samples smeared noise at `(x, 0.0, z)`.
    #[inline]
    #[must_use]
    pub fn smeared_noise_xz(&self, x: f64, z: f64, fudge_y_scale: f64) -> f32 {
        self.smeared_noise(x, 0.0, z, fudge_y_scale)
    }

    /// Samples smeared noise at `(x, y, 0.0)`.
    #[inline]
    #[must_use]
    pub fn smeared_noise_xy(&self, x: f64, y: f64, fudge_y_scale: f64) -> f32 {
        self.smeared_noise(x, y, 0.0, fudge_y_scale)
    }

    /// Samples an X/Z column of float-based `SmearedPerlinNoise` values.
    ///
    /// Coordinates and the vertical fudge stay in `f64`, as in vanilla. The
    /// gradients and interpolation remain in `f32` lanes.
    #[inline]
    #[must_use]
    pub fn smeared_noise_y_simd<const N: usize>(
        &self,
        original_x: f64,
        original_ys: Simd<f64, N>,
        original_z: f64,
        fudge_y_scale: f64,
    ) -> Simd<f32, N> {
        let x = steel_math::wrap(original_x) + self.xo;
        let ys = steel_math::wrap_simd(original_ys) + Simd::splat(self.yo);
        let z = steel_math::wrap(original_z) + self.zo;
        let floor_x = fast_floor(x);
        let floor_ys = fast_floor_simd::<f64, i32, N>(ys);
        let floor_z = fast_floor(z);
        let relative_ys = ys - floor_ys.cast();
        let zero = Simd::splat(0.0);
        let fudge_limits = (original_ys.simd_ge(zero) & original_ys.simd_lt(relative_ys))
            .select(original_ys, relative_ys);
        let fudge = (fudge_limits / Simd::splat(fudge_y_scale)
            + Simd::splat(f64::from(1.0e-7_f32)))
        .floor()
            * Simd::splat(fudge_y_scale);

        self.sample_and_lerp_y_simd(
            floor_x,
            floor_ys,
            floor_z,
            (x - f64::from(floor_x)) as f32,
            (relative_ys - fudge).cast(),
            (z - f64::from(floor_z)) as f32,
            relative_ys.cast(),
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "matches vanilla PerlinNoise.sampleAndLerp"
    )]
    fn sample_and_lerp(
        &self,
        x: i32,
        y: i32,
        z: i32,
        relative_x: f32,
        relative_y: f32,
        relative_z: f32,
        original_relative_y: f32,
    ) -> f32 {
        #[cfg(target_feature = "avx512f")]
        let [d000, d100, d010, d110, d001, d101, d011, d111] =
            self.corner_gradients_simd(x, y, z, relative_x, relative_y, relative_z);

        #[cfg(not(target_feature = "avx512f"))]
        let [d000, d100, d010, d110, d001, d101, d011, d111] = {
            let x1 = x.wrapping_add(1);
            let y1 = y.wrapping_add(1);
            let z1 = z.wrapping_add(1);
            let relative_x1 = relative_x - 1.0;
            let relative_y1 = relative_y - 1.0;
            let relative_z1 = relative_z - 1.0;

            let d000 = grad_dot_flat(&self.p, x, y, z, relative_x, relative_y, relative_z);
            let d100 = grad_dot_flat(&self.p, x1, y, z, relative_x1, relative_y, relative_z);
            let d010 = grad_dot_flat(&self.p, x, y1, z, relative_x, relative_y1, relative_z);
            let d110 = grad_dot_flat(&self.p, x1, y1, z, relative_x1, relative_y1, relative_z);
            let d001 = grad_dot_flat(&self.p, x, y, z1, relative_x, relative_y, relative_z1);
            let d101 = grad_dot_flat(&self.p, x1, y, z1, relative_x1, relative_y, relative_z1);
            let d011 = grad_dot_flat(&self.p, x, y1, z1, relative_x, relative_y1, relative_z1);
            let d111 = grad_dot_flat(&self.p, x1, y1, z1, relative_x1, relative_y1, relative_z1);
            [d000, d100, d010, d110, d001, d101, d011, d111]
        };
        let x_alpha = smoothstep(relative_x);
        let y_alpha = smoothstep(original_relative_y);
        let z_alpha = smoothstep(relative_z);
        let xz0 = lerp2(x_alpha, y_alpha, d000, d100, d010, d110);
        let xz1 = lerp2(x_alpha, y_alpha, d001, d101, d011, d111);
        lerp(z_alpha, xz0, xz1)
    }

    /// Batches the two Z faces while sharing their X/Y permutation lookups.
    #[cfg(any(test, target_feature = "avx512f"))]
    #[inline]
    fn corner_gradients_simd(
        &self,
        x: i32,
        y: i32,
        z: i32,
        relative_x: f32,
        relative_y: f32,
        relative_z: f32,
    ) -> [f32; 8] {
        let x = x as u8;
        let y = y as u8;
        let z = z as u8;
        let x0 = self.p[x as usize];
        let x1 = self.p[x.wrapping_add(1) as usize];
        let xy = [
            self.p[x0.wrapping_add(y) as usize],
            self.p[x1.wrapping_add(y) as usize],
            self.p[x0.wrapping_add(y).wrapping_add(1) as usize],
            self.p[x1.wrapping_add(y).wrapping_add(1) as usize],
        ];
        let xs = Simd::from_array([relative_x, relative_x - 1.0, relative_x, relative_x - 1.0]);
        let ys = Simd::from_array([relative_y, relative_y, relative_y - 1.0, relative_y - 1.0]);
        let sample_face = |face_z: u8, relative_z| {
            let gradients =
                xy.map(|xy| GRADIENT_F32[self.p[xy.wrapping_add(face_z) as usize] as usize & 15]);
            let gx = Simd::from_array(gradients.map(|g| g[0]));
            let gy = Simd::from_array(gradients.map(|g| g[1]));
            let gz = Simd::from_array(gradients.map(|g| g[2]));
            // Preserve all three terms and vanilla's float addition order.
            (gx * xs + gy * ys + gz * Simd::splat(relative_z)).to_array()
        };
        let low = sample_face(z, relative_z);
        let high = sample_face(z.wrapping_add(1), relative_z - 1.0);
        [
            low[0], low[1], low[2], low[3], high[0], high[1], high[2], high[3],
        ]
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors scalar sample_and_lerp with SIMD Y lanes"
    )]
    #[inline]
    fn sample_and_lerp_y_simd<const N: usize>(
        &self,
        x: i32,
        ys: Simd<i32, N>,
        z: i32,
        relative_x: f32,
        relative_ys: Simd<f32, N>,
        relative_z: f32,
        original_relative_ys: Simd<f32, N>,
    ) -> Simd<f32, N> {
        let x = x as u8;
        let z = z as u8;
        let x0 = self.p[x as usize];
        let x1 = self.p[x.wrapping_add(1) as usize];
        let ys = ys.cast::<u8>();

        let mut h000 = [0_usize; N];
        let mut h100 = [0_usize; N];
        let mut h010 = [0_usize; N];
        let mut h110 = [0_usize; N];
        let mut h001 = [0_usize; N];
        let mut h101 = [0_usize; N];
        let mut h011 = [0_usize; N];
        let mut h111 = [0_usize; N];

        for lane in 0..N {
            let y = ys[lane];
            let xy00 = self.p[x0.wrapping_add(y) as usize];
            let xy01 = self.p[x0.wrapping_add(y).wrapping_add(1) as usize];
            let xy10 = self.p[x1.wrapping_add(y) as usize];
            let xy11 = self.p[x1.wrapping_add(y).wrapping_add(1) as usize];
            h000[lane] = self.p[xy00.wrapping_add(z) as usize] as usize;
            h100[lane] = self.p[xy10.wrapping_add(z) as usize] as usize;
            h010[lane] = self.p[xy01.wrapping_add(z) as usize] as usize;
            h110[lane] = self.p[xy11.wrapping_add(z) as usize] as usize;
            h001[lane] = self.p[xy00.wrapping_add(z).wrapping_add(1) as usize] as usize;
            h101[lane] = self.p[xy10.wrapping_add(z).wrapping_add(1) as usize] as usize;
            h011[lane] = self.p[xy01.wrapping_add(z).wrapping_add(1) as usize] as usize;
            h111[lane] = self.p[xy11.wrapping_add(z).wrapping_add(1) as usize] as usize;
        }

        let relative_xs = Simd::splat(relative_x);
        let relative_zs = Simd::splat(relative_z);
        let one = Simd::splat(1.0);
        let relative_x1 = relative_xs - one;
        let relative_y1 = relative_ys - one;
        let relative_z1 = relative_zs - one;

        let d000 = grad_dot_simd(h000, relative_xs, relative_ys, relative_zs);
        let d100 = grad_dot_simd(h100, relative_x1, relative_ys, relative_zs);
        let d010 = grad_dot_simd(h010, relative_xs, relative_y1, relative_zs);
        let d110 = grad_dot_simd(h110, relative_x1, relative_y1, relative_zs);
        let d001 = grad_dot_simd(h001, relative_xs, relative_ys, relative_z1);
        let d101 = grad_dot_simd(h101, relative_x1, relative_ys, relative_z1);
        let d011 = grad_dot_simd(h011, relative_xs, relative_y1, relative_z1);
        let d111 = grad_dot_simd(h111, relative_x1, relative_y1, relative_z1);

        let x_alpha = smoothstep_simd(relative_xs);
        let y_alpha = smoothstep_simd(original_relative_ys);
        let z_alpha = smoothstep_simd(relative_zs);
        let xz0 = lerp2_simd(x_alpha, y_alpha, d000, d100, d010, d110);
        let xz1 = lerp2_simd(x_alpha, y_alpha, d001, d101, d011, d111);
        lerp_simd(z_alpha, xz0, xz1)
    }

    /// Calculate Perlin noise using SIMD vectors.
    #[inline]
    #[must_use]
    pub fn noise_simd<const N: usize>(
        &self,
        x: Simd<f32, N>,
        y: Simd<f32, N>,
        z: Simd<f32, N>,
    ) -> Simd<f32, N>
    where
        Simd<f32, N>: SimdFloat<Cast<i32> = Simd<i32, N>>
            + SimdPartialOrd<Mask = Mask<i32, N>>
            + SimdPartialEq<Mask = Mask<i32, N>>
            + std::ops::Add<Output = Simd<f32, N>>
            + std::ops::Sub<Output = Simd<f32, N>>
            + std::ops::Mul<Output = Simd<f32, N>>
            + std::ops::Neg<Output = Simd<f32, N>>,
    {
        let x = x + Simd::splat(self.xo).cast();
        let y = y + Simd::splat(self.yo).cast();
        let z = z + Simd::splat(self.zo).cast();

        let xf = fast_floor_simd::<f32, i32, N>(x);
        let yf = fast_floor_simd::<f32, i32, N>(y);
        let zf = fast_floor_simd::<f32, i32, N>(z);

        let xr = x - xf.cast();
        let yr = y - yf.cast();
        let zr = z - zf.cast();

        self.sample_and_lerp_simd(xf, yf, zf, xr, yr, zr, yr)
    }

    /// Sample noise at the given coordinates, accumulating partial derivatives.
    ///
    /// Returns the float noise value and adds the partial derivatives (dx, dy, dz)
    /// into `derivative_out`. Matches vanilla's `PerlinNoise.noiseWithDerivative`.
    #[must_use]
    pub fn noise_with_derivative(
        &self,
        x: f64,
        y: f64,
        z: f64,
        derivative_out: &mut [f32; 3],
    ) -> f32 {
        let x = steel_math::wrap(x) + self.xo;
        let y = steel_math::wrap(y) + self.yo;
        let z = steel_math::wrap(z) + self.zo;

        let xf = fast_floor(x);
        let yf = fast_floor(y);
        let zf = fast_floor(z);

        let xr = (x - f64::from(xf)) as f32;
        let yr = (y - f64::from(yf)) as f32;
        let zr = (z - f64::from(zf)) as f32;

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
    pub fn noise_with_y_scale(&self, x: f64, y: f64, z: f64, y_scale: f64, y_fudge: f64) -> f32 {
        let x = x + self.xo;
        let y = y + self.yo;
        let z = z + self.zo;

        let xf = fast_floor(x);
        let yf = fast_floor(y);
        let zf = fast_floor(z);

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
        self.sample_and_lerp(
            xf,
            yf,
            zf,
            xr as f32,
            (yr - yr_fudge) as f32,
            zr as f32,
            yr as f32,
        )
    }

    #[inline]
    fn p_simd<const N: usize>(&self, idx: Simd<u8, N>) -> Simd<u8, N> {
        let offset = idx.cast::<usize>();
        let p = Simd::splat(self.p.as_ptr()).wrapping_add(offset);
        // SAFETY: `idx` is a `Simd<u8, N>`, meaning each lane's index is at most 255.
        // `self.p` has length 256, so all offsets are guaranteed to be within bounds of `self.p`.
        unsafe { Simd::gather_ptr(p) }
    }

    /// Sample noise at grid point and interpolate.
    #[expect(clippy::too_many_arguments, reason = "matches vanilla signature")]
    fn sample_and_lerp_simd<const N: usize>(
        &self,
        x: Simd<i32, N>,
        y: Simd<i32, N>,
        z: Simd<i32, N>,
        xr: Simd<f32, N>,
        yr: Simd<f32, N>,
        zr: Simd<f32, N>,
        yr_original: Simd<f32, N>,
    ) -> Simd<f32, N>
    where
        Simd<f32, N>: std::ops::Mul<Output = Simd<f32, N>>
            + std::ops::Add<Output = Simd<f32, N>>
            + std::ops::Sub<Output = Simd<f32, N>>
            + std::ops::Neg<Output = Simd<f32, N>>,
    {
        let x = x.cast::<u8>();
        let y = y.cast::<u8>();
        let z = z.cast::<u8>();
        // Get permutation indices for the 8 corners
        let x0 = self.p_simd(x);
        let x1 = self.p_simd(x + Simd::splat(1));

        let xy00 = self.p_simd(x0 + y);
        let xy01 = self.p_simd(x0 + y + Simd::splat(1));
        let xy10 = self.p_simd(x1 + y);
        let xy11 = self.p_simd(x1 + y + Simd::splat(1));

        let h000 = self.p_simd(xy00 + z).cast::<usize>().to_array();
        let h100 = self.p_simd(xy10 + z).cast::<usize>().to_array();
        let h010 = self.p_simd(xy01 + z).cast::<usize>().to_array();
        let h110 = self.p_simd(xy11 + z).cast::<usize>().to_array();
        let h001 = self
            .p_simd(xy00 + z + Simd::splat(1))
            .cast::<usize>()
            .to_array();
        let h101 = self
            .p_simd(xy10 + z + Simd::splat(1))
            .cast::<usize>()
            .to_array();
        let h011 = self
            .p_simd(xy01 + z + Simd::splat(1))
            .cast::<usize>()
            .to_array();
        let h111 = self
            .p_simd(xy11 + z + Simd::splat(1))
            .cast::<usize>()
            .to_array();

        // Calculate gradient dot products at each corner
        let d000 = grad_dot_simd(h000, xr, yr, zr);
        let d100 = grad_dot_simd(h100, xr - Simd::splat(1.0), yr, zr);
        let d010 = grad_dot_simd(h010, xr, yr - Simd::splat(1.0), zr);
        let d110 = grad_dot_simd(h110, xr - Simd::splat(1.0), yr - Simd::splat(1.0), zr);
        let d001 = grad_dot_simd(h001, xr, yr, zr - Simd::splat(1.0));
        let d101 = grad_dot_simd(h101, xr - Simd::splat(1.0), yr, zr - Simd::splat(1.0));
        let d011 = grad_dot_simd(h011, xr, yr - Simd::splat(1.0), zr - Simd::splat(1.0));
        let d111 = grad_dot_simd(
            h111,
            xr - Simd::splat(1.0),
            yr - Simd::splat(1.0),
            zr - Simd::splat(1.0),
        );

        // Apply smoothstep interpolation
        let x_alpha = smoothstep_simd(xr);
        let y_alpha = smoothstep_simd(yr_original);
        let z_alpha = smoothstep_simd(zr);

        lerp3_simd(
            x_alpha, y_alpha, z_alpha, d000, d100, d010, d110, d001, d101, d011, d111,
        )
    }

    /// Generic N-lane form of the Y-scaled float sampler. Each lane runs the
    /// exact per-lane math of the scalar [`Self::noise_with_y_scale`], so any
    /// supported lane width yields bit-identical per-lane results — only the
    /// SIMD batch size changes.
    #[inline]
    #[must_use]
    pub fn noise_with_y_scale_simd<const N: usize>(
        &self,
        x: f64,
        ys: Simd<f64, N>,
        z: f64,
        y_scale: f64,
        y_fudges: Simd<f64, N>,
    ) -> Simd<f32, N> {
        // Shared x/z offset and floor
        let x = x + self.xo;
        let z = z + self.zo;
        let xf = fast_floor(x);
        let zf = fast_floor(z);
        let xr = x - f64::from(xf);
        let zr = z - f64::from(zf);

        // Per-lane y offset and floor
        let ys = ys + Simd::splat(self.yo);
        let ys_floor = fast_floor_simd::<f64, i32, N>(ys);
        let yrs = ys - ys_floor.cast();

        // Y fudge (per-lane)
        let yr_fudge: Simd<f64, N> = if y_scale == 0.0 {
            Simd::splat(0.0)
        } else {
            let y_scale_v: Simd<f64, N> = Simd::splat(y_scale);
            let zero: Simd<f64, N> = Simd::splat(0.0);
            let mask = y_fudges.simd_ge(zero) & y_fudges.simd_lt(yrs);
            let fudge_limits = mask.select(y_fudges, yrs);
            let epsilon: Simd<f64, N> = Simd::splat(f64::from(1.0e-7_f32));
            ((fudge_limits / y_scale_v) + epsilon).floor() * y_scale_v
        };

        let yrs_adjusted = yrs - yr_fudge;

        self.sample_and_lerp_y_simd(
            xf,
            ys_floor,
            zf,
            xr as f32,
            yrs_adjusted.cast(),
            zr as f32,
            yrs.cast(),
        )
    }

    /// Sample noise at grid point, interpolate, and accumulate derivatives.
    #[expect(clippy::too_many_arguments, reason = "matches vanilla signature")]
    fn sample_with_derivative(
        &self,
        x: i32,
        y: i32,
        z: i32,
        xr: f32,
        yr: f32,
        zr: f32,
        derivative_out: &mut [f32; 3],
    ) -> f32 {
        let x = x as u8;
        let y = y as u8;
        let z = z as u8;

        let x0 = self.p[x as usize];
        let x1 = self.p[x.wrapping_add(1) as usize];
        let xy00 = self.p[x0.wrapping_add(y) as usize];
        let xy01 = self.p[x0.wrapping_add(y).wrapping_add(1) as usize];
        let xy10 = self.p[x1.wrapping_add(y) as usize];
        let xy11 = self.p[x1.wrapping_add(y).wrapping_add(1) as usize];

        let h000 = self.p[xy00.wrapping_add(z) as usize] as usize;
        let h100 = self.p[xy10.wrapping_add(z) as usize] as usize;
        let h010 = self.p[xy01.wrapping_add(z) as usize] as usize;
        let h110 = self.p[xy11.wrapping_add(z) as usize] as usize;
        let h001 = self.p[xy00.wrapping_add(z).wrapping_add(1) as usize] as usize;
        let h101 = self.p[xy10.wrapping_add(z).wrapping_add(1) as usize] as usize;
        let h011 = self.p[xy01.wrapping_add(z).wrapping_add(1) as usize] as usize;
        let h111 = self.p[xy11.wrapping_add(z).wrapping_add(1) as usize] as usize;

        let g000 = Simd::from_array(GRADIENT_F32[h000 & 15]);
        let g100 = Simd::from_array(GRADIENT_F32[h100 & 15]);
        let g010 = Simd::from_array(GRADIENT_F32[h010 & 15]);
        let g110 = Simd::from_array(GRADIENT_F32[h110 & 15]);
        let g001 = Simd::from_array(GRADIENT_F32[h001 & 15]);
        let g101 = Simd::from_array(GRADIENT_F32[h101 & 15]);
        let g011 = Simd::from_array(GRADIENT_F32[h011 & 15]);
        let g111 = Simd::from_array(GRADIENT_F32[h111 & 15]);

        // Gradient dot products at each corner
        let d000 = grad_dot(h000, xr, yr, zr);
        let d100 = grad_dot(h100, xr - 1.0, yr, zr);
        let d010 = grad_dot(h010, xr, yr - 1.0, zr);
        let d110 = grad_dot(h110, xr - 1.0, yr - 1.0, zr);
        let d001 = grad_dot(h001, xr, yr, zr - 1.0);
        let d101 = grad_dot(h101, xr - 1.0, yr, zr - 1.0);
        let d011 = grad_dot(h011, xr, yr - 1.0, zr - 1.0);
        let d111 = grad_dot(h111, xr - 1.0, yr - 1.0, zr - 1.0);

        let alpha_x = smoothstep(xr);
        let alpha_y = smoothstep(yr);
        let alpha_z = smoothstep(zr);

        // Interpolate gradient components for direct derivative contribution
        let d1_v = lerp3_simd(
            Simd::splat(alpha_x),
            Simd::splat(alpha_y),
            Simd::splat(alpha_z),
            g000,
            g100,
            g010,
            g110,
            g001,
            g101,
            g011,
            g111,
        );

        // Smoothstep correction terms via differences
        let d2x = lerp2(
            alpha_y,
            alpha_z,
            d100 - d000,
            d110 - d010,
            d101 - d001,
            d111 - d011,
        );
        let d2y = lerp2(
            alpha_z,
            alpha_x,
            d010 - d000,
            d011 - d001,
            d110 - d100,
            d111 - d101,
        );
        let d2z = lerp2(
            alpha_x,
            alpha_y,
            d001 - d000,
            d101 - d100,
            d011 - d010,
            d111 - d110,
        );

        let x_sd = smoothstep_derivative(xr);
        let y_sd = smoothstep_derivative(yr);
        let z_sd = smoothstep_derivative(zr);

        // Accumulate derivatives (vanilla uses +=)
        derivative_out[0] += d1_v[0] + x_sd * d2x;
        derivative_out[1] += d1_v[1] + y_sd * d2y;
        derivative_out[2] += d1_v[2] + z_sd * d2z;
        lerp3(
            alpha_x, alpha_y, alpha_z, d000, d100, d010, d110, d001, d101, d011, d111,
        )
    }
}

/// Matches vanilla's shared permutation lookup and typed gradient dot product.
#[inline]
fn gradient_hash(p: &[u8; 256], px: i32, py: i32, pz: i32) -> usize {
    let qx = (px & 0xFF) as u8;
    let qy = (py & 0xFF) as u8;
    let qz = (pz & 0xFF) as u8;
    let a = p[qx as usize];
    let b = p[a.wrapping_add(qy) as usize];
    p[b.wrapping_add(qz) as usize] as usize
}

#[inline]
fn grad_dot_flat(p: &[u8; 256], px: i32, py: i32, pz: i32, fx: f32, fy: f32, fz: f32) -> f32 {
    grad_dot(gradient_hash(p, px, py, pz), fx, fy, fz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::xoroshiro::Xoroshiro;
    use std::simd::f64x4;

    #[test]
    fn corner_gradient_batches_match_scalar_at_permutation_boundaries() {
        let mut random = Xoroshiro::from_seed(42);
        let noise = ImprovedNoise::new(&mut random);
        for x in [-257_i32, -1, 0, 255, 256, i32::MAX] {
            for y in [-1_i32, 0, 255, i32::MAX] {
                for z in [-1_i32, 0, 255, i32::MAX] {
                    for [rx, ry, rz] in [[0.0_f32, 0.0, 0.0], [0.125, -0.25, 0.75], [0.9, 0.7, 0.3]]
                    {
                        let actual = noise.corner_gradients_simd(x, y, z, rx, ry, rz);
                        for (corner, actual) in actual.into_iter().enumerate() {
                            let dx = (corner & 1) as i32;
                            let dy = ((corner >> 1) & 1) as i32;
                            let dz = ((corner >> 2) & 1) as i32;
                            let expected = grad_dot_flat(
                                &noise.p,
                                x.wrapping_add(dx),
                                y.wrapping_add(dy),
                                z.wrapping_add(dz),
                                rx - dx as f32,
                                ry - dy as f32,
                                rz - dz as f32,
                            );
                            assert_eq!(
                                actual.to_bits(),
                                expected.to_bits(),
                                "({x}, {y}, {z}), corner {corner}"
                            );
                        }
                    }
                }
            }
        }
    }

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

                    let simd_result = noise.noise_with_y_scale_simd(
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
    fn smeared_noise_y_simd_matches_scalar() {
        let mut rng = Xoroshiro::from_seed(42);
        let noise = ImprovedNoise::new(&mut rng);
        let ys = [-64.0, -56.0, -48.0, -40.0, -32.0, -24.0, -16.0, -8.0];
        let simd = noise.smeared_noise_y_simd(
            20_000_068.0,
            std::simd::f64x8::from_array(ys),
            -19_999_796.0,
            5475.296,
        );

        for (&y, &value) in ys.iter().zip(simd.as_array()) {
            assert_eq!(
                value.to_bits(),
                noise
                    .smeared_noise(20_000_068.0, y, -19_999_796.0, 5475.296)
                    .to_bits(),
                "Y={y}"
            );
        }
    }

    #[test]
    fn test_noise_y_simd_matches_scalar() {
        let mut rng = Xoroshiro::from_seed(42);
        let noise = ImprovedNoise::new(&mut rng);
        let columns = [
            (0.0, 0.0, [0.0, 1.25, -5.5, 1000.75]),
            (
                255.25 - noise.xo,
                -256.25 - noise.zo,
                [
                    255.5 - noise.yo,
                    256.5 - noise.yo,
                    -1.5 - noise.yo,
                    -256.5 - noise.yo,
                ],
            ),
            (33_554_432.0, -33_554_432.0, [64.0, 64.5, 65.0, 65.5]),
        ];

        for (x, z, ys) in columns {
            let simd = noise.noise_y_simd(x, f64x4::from_array(ys), z);
            for (lane, y) in ys.into_iter().enumerate() {
                assert_eq!(simd[lane].to_bits(), noise.noise(x, y, z).to_bits());
            }
        }
    }

    #[test]
    fn test_zero_axis_helpers_match_full_noise() {
        let mut rng = Xoroshiro::from_seed(12_345);
        let noise = ImprovedNoise::new(&mut rng);
        let samples = [
            (0.0, 0.0),
            (1.25, -30.75),
            (-1000.0, 4096.5),
            (33_554_431.5, -33_554_432.25),
            (-0.000_000_1, 0.000_000_1),
        ];

        for &(a, b) in &samples {
            assert_eq!(noise.noise_xz(a, b), noise.noise(a, 0.0, b));
            assert_eq!(noise.noise_xy(a, b), noise.noise(a, b, 0.0));
        }
    }

    #[test]
    fn test_zero_axis_smeared_helpers_match_full_noise() {
        let mut rng = Xoroshiro::from_seed(12_345);
        let noise = ImprovedNoise::new(&mut rng);
        let fudge_y_scale = 5475.296;
        let samples = [(0.0, 0.0), (1.25, -30.75), (-1000.0, 4096.5)];

        for &(a, b) in &samples {
            assert_eq!(
                noise.smeared_noise_xz(a, b, fudge_y_scale),
                noise.smeared_noise(a, 0.0, b, fudge_y_scale),
            );
            assert_eq!(
                noise.smeared_noise_xy(a, b, fudge_y_scale),
                noise.smeared_noise(a, b, 0.0, fudge_y_scale),
            );
        }
    }

    #[test]
    fn test_noise_with_y_scale_simd8_matches_scalar() {
        use std::simd::f64x8;

        let mut rng = Xoroshiro::from_seed(42);
        let noise = ImprovedNoise::new(&mut rng);

        let test_x_zs: &[(f64, f64)] = &[
            (0.0, 0.0),
            (1.5, 3.7),
            (-5.2, 100.3),
            (0.001, -0.001),
            (1000.0, -500.0),
        ];
        let test_ys: &[[f64; 8]] = &[
            [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
            [64.0, 64.5, 65.0, 65.5, 66.0, 66.5, 67.0, 67.5],
            [-5.0, -2.5, 0.0, 2.5, 5.0, 7.5, 10.0, 12.5],
            [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0],
            [-100.0, -50.0, -25.0, -10.0, 10.0, 25.0, 50.0, 100.0],
        ];
        let y_scales = [0.0, 1.0, 8.0];

        for &(x, z) in test_x_zs {
            for ys in test_ys {
                for &y_scale in &y_scales {
                    let y_fudges: [f64; 8] = if y_scale == 0.0 { [0.0; 8] } else { *ys };

                    let simd_result = noise.noise_with_y_scale_simd(
                        x,
                        f64x8::from_array(*ys),
                        z,
                        y_scale,
                        f64x8::from_array(y_fudges),
                    );

                    for i in 0..8 {
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
        assert_eq!(noise1.xo, noise2.xo);
        assert_eq!(noise1.yo, noise2.yo);
        assert_eq!(noise1.zo, noise2.zo);
        assert_eq!(noise1.p, noise2.p);

        // Same coordinates should produce same values
        let v1 = noise1.noise(100.0, 64.0, 100.0);
        let v2 = noise2.noise(100.0, 64.0, 100.0);
        assert!((v1 - v2).abs() < 1e-15);
    }

    #[test]
    fn scalar_noise_wraps_corner_coordinates_at_i32_max() {
        let mut rng = Xoroshiro::from_seed(42);
        let mut noise = ImprovedNoise::new(&mut rng);
        noise.xo = 0.0;
        noise.yo = 0.0;
        noise.zo = 0.0;

        let _ = noise.noise(
            f64::from(i32::MAX),
            f64::from(i32::MAX),
            f64::from(i32::MAX),
        );
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
