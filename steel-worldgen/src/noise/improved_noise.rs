//! Improved Perlin noise implementation matching vanilla Minecraft's ImprovedNoise.java
//!
//! This is the base noise generator used by `PerlinNoise` for octave-based noise.

use crate::random::Random;
use std::ops;
use std::simd::Simd;
use std::simd::cmp::{SimdPartialEq, SimdPartialOrd};
#[cfg(target_feature = "avx512f")]
use std::simd::f64x4;
use std::simd::num::{SimdFloat, SimdInt, SimdUint};
use std::simd::ptr::SimdConstPtr;
use std::simd::{Mask, Select, SimdCast, SimdElement, StdFloat};
use steel_math::{
    GRADIENT, fast_floor, fast_floor_simd, grad_dot, grad_dot_simd, lerp2, lerp3, lerp3_simd,
    smoothstep, smoothstep_derivative, smoothstep_simd,
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

    /// Sample noise at the given coordinates.
    ///
    /// This is the standard 3D Perlin noise sampling without Y scaling.
    #[inline]
    #[must_use]
    pub fn noise(&self, x: f64, y: f64, z: f64) -> f64 {
        let x = x + self.xo;
        let y = y + self.yo;
        let z = z + self.zo;

        let xf = fast_floor(x);
        let yf = fast_floor(y);
        let zf = fast_floor(z);

        let xr = x - f64::from(xf);
        let yr = y - f64::from(yf);
        let zr = z - f64::from(zf);

        self.sample_and_lerp(xf, yf, zf, xr, yr, zr, yr)
    }

    /// Calculate Perlin noise using SIMD vectors.
    #[inline]
    #[must_use]
    pub fn noise_simd<F, const N: usize>(
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
            + ops::Neg<Output = Simd<F, N>>,
    {
        let x = x + Simd::splat(self.xo).cast();
        let y = y + Simd::splat(self.yo).cast();
        let z = z + Simd::splat(self.zo).cast();

        let xf = fast_floor_simd::<F, i32, N>(x);
        let yf = fast_floor_simd::<F, i32, N>(y);
        let zf = fast_floor_simd::<F, i32, N>(z);

        let xr = x - xf.cast();
        let yr = y - yf.cast();
        let zr = z - zf.cast();

        self.sample_and_lerp_simd(xf, yf, zf, xr, yr, zr, yr)
    }

    /// Sample noise at `(x, 0.0, z)`.
    #[inline]
    #[must_use]
    pub fn noise_xz(&self, x: f64, z: f64) -> f64 {
        let x = x + self.xo;
        let z = z + self.zo;

        let xf = fast_floor(x);
        let zf = fast_floor(z);

        let xr = x - f64::from(xf);
        let zr = z - f64::from(zf);

        self.sample_and_lerp(
            xf,
            self.yo_floor,
            zf,
            xr,
            self.yo_fraction,
            zr,
            self.yo_fraction,
        )
    }

    /// Sample noise at `(x, y, 0.0)`.
    #[inline]
    #[must_use]
    pub fn noise_xy(&self, x: f64, y: f64) -> f64 {
        let x = x + self.xo;
        let y = y + self.yo;

        let xf = fast_floor(x);
        let yf = fast_floor(y);

        let xr = x - f64::from(xf);
        let yr = y - f64::from(yf);

        self.sample_and_lerp(xf, yf, self.zo_floor, xr, yr, self.zo_fraction, yr)
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

        let xf = fast_floor(x);
        let yf = fast_floor(y);
        let zf = fast_floor(z);

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
        self.sample_and_lerp(xf, yf, zf, xr, yr - yr_fudge, zr, yr)
    }

    /// Sample noise at grid point and interpolate.
    ///
    /// The 8 corner gradient-dot products are evaluated as 2 × `f64x4` so the
    /// per-lane math stays identical to the scalar path (`((gx*xr) + (gy*yr)) + (gz*zr)`),
    /// which preserves bit-identical output. Inspired by C2ME's `c2me-opts-math`
    /// flat-gradient SIMD form.
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
        let x = x as u8;
        let y = y as u8;
        let z = z as u8;
        // Get permutation indices for the 8 corners
        let x0 = self.p[x as usize];
        let x1 = self.p[x.wrapping_add(1) as usize];

        let xy00 = self.p[x0.wrapping_add(y) as usize];
        let xy01 = self.p[x0.wrapping_add(y).wrapping_add(1) as usize];
        let xy10 = self.p[x1.wrapping_add(y) as usize];
        let xy11 = self.p[x1.wrapping_add(y).wrapping_add(1) as usize];

        let (d000, d100, d010, d110, d001, d101, d011, d111) = {
            #[cfg(target_feature = "avx512f")]
            {
                // Hashes for the z-face and z+1-face, in (000,100,010,110) order.
                let h_z0 = [
                    self.p[xy00.wrapping_add(z) as usize] as usize,
                    self.p[xy10.wrapping_add(z) as usize] as usize,
                    self.p[xy01.wrapping_add(z) as usize] as usize,
                    self.p[xy11.wrapping_add(z) as usize] as usize,
                ];
                let h_z1 = [
                    self.p[xy00.wrapping_add(z).wrapping_add(1) as usize] as usize,
                    self.p[xy10.wrapping_add(z).wrapping_add(1) as usize] as usize,
                    self.p[xy01.wrapping_add(z).wrapping_add(1) as usize] as usize,
                    self.p[xy11.wrapping_add(z).wrapping_add(1) as usize] as usize,
                ];

                let xr_v = f64x4::from_array([xr, xr - 1.0, xr, xr - 1.0]);
                let yr_v = f64x4::from_array([yr, yr, yr - 1.0, yr - 1.0]);
                let zr_v0 = f64x4::splat(zr);
                let zr_v1 = f64x4::splat(zr - 1.0);

                let [d000, d100, d010, d110] = grad_dot_simd(h_z0, xr_v, yr_v, zr_v0).to_array();
                let [d001, d101, d011, d111] = grad_dot_simd(h_z1, xr_v, yr_v, zr_v1).to_array();
                (d000, d100, d010, d110, d001, d101, d011, d111)
            }

            #[cfg(not(target_feature = "avx512f"))]
            {
                // Calculate gradient dot products at each corner
                let d000 = grad_dot(self.p[xy00.wrapping_add(z) as usize] as usize, xr, yr, zr);
                let d100 = grad_dot(
                    self.p[xy10.wrapping_add(z) as usize] as usize,
                    xr - 1.0,
                    yr,
                    zr,
                );
                let d010 = grad_dot(
                    self.p[xy01.wrapping_add(z) as usize] as usize,
                    xr,
                    yr - 1.0,
                    zr,
                );
                let d110 = grad_dot(
                    self.p[xy11.wrapping_add(z) as usize] as usize,
                    xr - 1.0,
                    yr - 1.0,
                    zr,
                );
                let d001 = grad_dot(
                    self.p[xy00.wrapping_add(z).wrapping_add(1) as usize] as usize,
                    xr,
                    yr,
                    zr - 1.0,
                );
                let d101 = grad_dot(
                    self.p[xy10.wrapping_add(z).wrapping_add(1) as usize] as usize,
                    xr - 1.0,
                    yr,
                    zr - 1.0,
                );
                let d011 = grad_dot(
                    self.p[xy01.wrapping_add(z).wrapping_add(1) as usize] as usize,
                    xr,
                    yr - 1.0,
                    zr - 1.0,
                );
                let d111 = grad_dot(
                    self.p[xy11.wrapping_add(z).wrapping_add(1) as usize] as usize,
                    xr - 1.0,
                    yr - 1.0,
                    zr - 1.0,
                );
                (d000, d100, d010, d110, d001, d101, d011, d111)
            }
        };
        // Apply smoothstep interpolation
        let x_alpha = smoothstep(xr);
        let y_alpha = smoothstep(yr_original);
        let z_alpha = smoothstep(zr);

        lerp3(
            x_alpha, y_alpha, z_alpha, d000, d100, d010, d110, d001, d101, d011, d111,
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
    fn sample_and_lerp_simd<F, const N: usize>(
        &self,
        x: Simd<i32, N>,
        y: Simd<i32, N>,
        z: Simd<i32, N>,
        xr: Simd<F, N>,
        yr: Simd<F, N>,
        zr: Simd<F, N>,
        yr_original: Simd<F, N>,
    ) -> Simd<F, N>
    where
        F: SimdElement + SimdCast,
        Simd<F, N>: ops::Mul<Output = Simd<F, N>>
            + ops::Add<Output = Simd<F, N>>
            + ops::Sub<Output = Simd<F, N>>
            + ops::Neg<Output = Simd<F, N>>,
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
        let d100 = grad_dot_simd(h100, xr - Simd::splat(1.0).cast::<F>(), yr, zr);
        let d010 = grad_dot_simd(h010, xr, yr - Simd::splat(1.0).cast::<F>(), zr);
        let d110 = grad_dot_simd(
            h110,
            xr - Simd::splat(1.0).cast::<F>(),
            yr - Simd::splat(1.0).cast::<F>(),
            zr,
        );
        let d001 = grad_dot_simd(h001, xr, yr, zr - Simd::splat(1.0).cast::<F>());
        let d101 = grad_dot_simd(
            h101,
            xr - Simd::splat(1.0).cast::<F>(),
            yr,
            zr - Simd::splat(1.0).cast::<F>(),
        );
        let d011 = grad_dot_simd(
            h011,
            xr,
            yr - Simd::splat(1.0).cast::<F>(),
            zr - Simd::splat(1.0).cast::<F>(),
        );
        let d111 = grad_dot_simd(
            h111,
            xr - Simd::splat(1.0).cast::<F>(),
            yr - Simd::splat(1.0).cast::<F>(),
            zr - Simd::splat(1.0).cast::<F>(),
        );

        // Apply smoothstep interpolation
        let x_alpha = smoothstep_simd(xr);
        let y_alpha = smoothstep_simd(yr_original);
        let z_alpha = smoothstep_simd(zr);

        lerp3_simd(
            x_alpha, y_alpha, z_alpha, d000, d100, d010, d110, d001, d101, d011, d111,
        )
    }

    /// Generic N-lane form of [`Self::noise_with_y_scale_4x`]. Each lane runs the
    /// exact per-lane math of the scalar [`Self::noise_with_y_scale`], so any
    /// supported lane width yields bit-identical per-lane results — only the
    /// SIMD batch size changes. `f64x4` ≡ `noise_with_y_scale_simd::<4>`.
    #[inline]
    #[must_use]
    pub fn noise_with_y_scale_simd<const N: usize>(
        &self,
        x: f64,
        ys: Simd<f64, N>,
        z: f64,
        y_scale: f64,
        y_fudges: Simd<f64, N>,
    ) -> Simd<f64, N> {
        // Shared x/z offset and floor
        let x = x + self.xo;
        let z = z + self.zo;
        let xf = fast_floor(x);
        let zf = fast_floor(z);
        let xr = x - f64::from(xf);
        let zr = z - f64::from(zf);

        // Per-lane y offset and floor
        let ys = ys + Simd::splat(self.yo);
        let ys_floor = ys.floor();
        let yrs = ys - ys_floor;

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

        self.sample_and_lerp_y_simd(xf, zf, xr, zr, ys_floor, yrs_adjusted, yrs)
    }

    /// Vectorized sample-and-lerp for N Y values sharing x/z grid position.
    /// Generic counterpart of [`Self::sample_and_lerp_4x`].
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors scalar sample_and_lerp with Nx SIMD y-batching"
    )]
    #[inline]
    fn sample_and_lerp_y_simd<const N: usize>(
        &self,
        xf: i32,
        zf: i32,
        xr: f64,
        zr: f64,
        ys_floor: Simd<f64, N>,
        yrs: Simd<f64, N>,
        yrs_original: Simd<f64, N>,
    ) -> Simd<f64, N> {
        let xf = xf as u8;
        let zf = zf as u8;
        // Shared x permutation lookups (2 instead of 2×N)
        let x0 = self.p[xf as usize];
        let x1 = self.p[xf.wrapping_add(1) as usize];

        let yf = ys_floor.cast::<i32>().cast();

        // Per-lane y-dependent permutation lookups
        let mut h000 = [0usize; N];
        let mut h100 = [0usize; N];
        let mut h010 = [0usize; N];
        let mut h110 = [0usize; N];
        let mut h001 = [0usize; N];
        let mut h101 = [0usize; N];
        let mut h011 = [0usize; N];
        let mut h111 = [0usize; N];

        for i in 0..N {
            let y = yf[i];
            let xy00 = self.p[x0.wrapping_add(y) as usize];
            let xy01 = self.p[x0.wrapping_add(y).wrapping_add(1) as usize];
            let xy10 = self.p[x1.wrapping_add(y) as usize];
            let xy11 = self.p[x1.wrapping_add(y).wrapping_add(1) as usize];
            h000[i] = self.p[xy00.wrapping_add(zf) as usize] as usize;
            h100[i] = self.p[xy10.wrapping_add(zf) as usize] as usize;
            h010[i] = self.p[xy01.wrapping_add(zf) as usize] as usize;
            h110[i] = self.p[xy11.wrapping_add(zf) as usize] as usize;
            h001[i] = self.p[xy00.wrapping_add(zf).wrapping_add(1) as usize] as usize;
            h101[i] = self.p[xy10.wrapping_add(zf).wrapping_add(1) as usize] as usize;
            h011[i] = self.p[xy01.wrapping_add(zf).wrapping_add(1) as usize] as usize;
            h111[i] = self.p[xy11.wrapping_add(zf).wrapping_add(1) as usize] as usize;
        }

        // Vectorized gradient dot products
        let xr_v: Simd<f64, N> = Simd::splat(xr);
        let zr_v: Simd<f64, N> = Simd::splat(zr);
        let one: Simd<f64, N> = Simd::splat(1.0);
        let xr_m1 = xr_v - one;
        let yr_m1 = yrs - one;
        let zr_m1 = zr_v - one;

        let d000 = grad_dot_simd(h000, xr_v, yrs, zr_v);
        let d100 = grad_dot_simd(h100, xr_m1, yrs, zr_v);
        let d010 = grad_dot_simd(h010, xr_v, yr_m1, zr_v);
        let d110 = grad_dot_simd(h110, xr_m1, yr_m1, zr_v);
        let d001 = grad_dot_simd(h001, xr_v, yrs, zr_m1);
        let d101 = grad_dot_simd(h101, xr_m1, yrs, zr_m1);
        let d011 = grad_dot_simd(h011, xr_v, yr_m1, zr_m1);
        let d111 = grad_dot_simd(h111, xr_m1, yr_m1, zr_m1);

        // Smoothstep — x and z are shared across lanes
        let x_alpha: Simd<f64, N> = Simd::splat(smoothstep(xr));
        let y_alpha = smoothstep_simd(yrs_original);
        let z_alpha: Simd<f64, N> = Simd::splat(smoothstep(zr));

        lerp3_simd(
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

        let g000 = Simd::from_array(GRADIENT[h000 & 15]);
        let g100 = Simd::from_array(GRADIENT[h100 & 15]);
        let g010 = Simd::from_array(GRADIENT[h010 & 15]);
        let g110 = Simd::from_array(GRADIENT[h110 & 15]);
        let g001 = Simd::from_array(GRADIENT[h001 & 15]);
        let g101 = Simd::from_array(GRADIENT[h101 & 15]);
        let g011 = Simd::from_array(GRADIENT[h011 & 15]);
        let g111 = Simd::from_array(GRADIENT[h111 & 15]);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::xoroshiro::Xoroshiro;
    use std::simd::f64x4;

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
    fn test_noise_simd_matches_scalar() {
        let mut rng = Xoroshiro::from_seed(42);
        let noise = ImprovedNoise::new(&mut rng);

        let batches = [
            (
                [0.0, 1.25, -5.5, 1000.75],
                [0.0, 64.5, -20.25, 255.75],
                [0.0, -30.75, 4096.5, -1000.25],
            ),
            (
                [
                    255.25 - noise.xo,
                    256.25 - noise.xo,
                    -1.75 - noise.xo,
                    -256.25 - noise.xo,
                ],
                [
                    255.5 - noise.yo,
                    256.5 - noise.yo,
                    -1.5 - noise.yo,
                    -256.5 - noise.yo,
                ],
                [
                    255.75 - noise.zo,
                    256.75 - noise.zo,
                    -1.25 - noise.zo,
                    -256.25 - noise.zo,
                ],
            ),
        ];

        for (xs, ys, zs) in batches {
            let simd = noise.noise_simd(
                f64x4::from_array(xs),
                f64x4::from_array(ys),
                f64x4::from_array(zs),
            );
            for i in 0..4 {
                let scalar = noise.noise(xs[i], ys[i], zs[i]);
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
    fn test_noise_matches_zero_y_scale_path() {
        let mut rng = Xoroshiro::from_seed(42);
        let noise = ImprovedNoise::new(&mut rng);

        for (x, y, z) in [
            (0.0, 0.0, 0.0),
            (1.25, 64.5, -30.75),
            (-1000.0, -20.25, 4096.5),
        ] {
            assert!(
                (noise.noise(x, y, z) - noise.noise_with_y_scale(x, y, z, 0.0, 0.0)).abs() < 1e-15
            );
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
            #[expect(
                clippy::float_cmp,
                reason = "zero-axis helpers must be bit-identical to the full scalar path"
            )]
            {
                assert_eq!(noise.noise_xz(a, b), noise.noise(a, 0.0, b));
                assert_eq!(noise.noise_xy(a, b), noise.noise(a, b, 0.0));
            }
        }
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
