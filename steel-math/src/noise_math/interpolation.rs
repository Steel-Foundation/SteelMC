use core::simd::{Select, Simd, cmp::SimdPartialOrd};
use std::{
    ops,
    simd::{SimdCast, SimdElement, num::SimdFloat},
};

/// Clamped linear interpolation.
///
/// Clamps the interpolation factor to [0, 1] before interpolating.
///
/// Java reference: `Mth.clampedLerp(double, double, double)`.
/// Note: Vanilla's parameter order is `(factor, min, max)`, ours is `(min, max, factor)`.
#[inline]
#[must_use]
pub fn clamped_lerp<T>(min: T, max: T, factor: T) -> T
where
    T: ops::Add<Output = T>
        + Copy
        + From<f32>
        + ops::Mul<Output = T>
        + PartialOrd
        + ops::Sub<Output = T>,
{
    if factor < T::from(0.0) {
        min
    } else if factor > T::from(1.0) {
        max
    } else {
        lerp(factor, min, max)
    }
}

/// Clamped lerp for N lanes.
#[inline]
#[must_use]
pub fn clamped_lerp_simd<const N: usize>(
    min: Simd<f64, N>,
    max: Simd<f64, N>,
    factor: Simd<f64, N>,
) -> Simd<f64, N> {
    let zero = Simd::splat(0.0);
    let one = Simd::splat(1.0);
    let below = factor.simd_lt(zero);
    let above = factor.simd_gt(one);

    // lerp result for the middle case
    let lerped = min + factor * (max - min);

    // Select: below zero → min, above one → max, otherwise → lerped
    let result = below.select(min, lerped);
    above.select(max, result)
}

/// Clamp a value to the range [min, max].
///
/// Java reference: `Mth.clamp(double, double, double)`
#[inline]
#[must_use]
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Clamp a value to the range [min, max] (i32 version).
#[inline]
#[must_use]
pub const fn clamp_i32(value: i32, min: i32, max: i32) -> i32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}
/// Inverse linear interpolation (find the factor t such that lerp(t, a, b) == value).
///
/// Java reference: `Mth.inverseLerp(double, double, double)`
#[inline]
#[must_use]
pub fn inverse_lerp(value: f64, a: f64, b: f64) -> f64 {
    (value - a) / (b - a)
}

/// Linear interpolation.
///
/// Formula: a + alpha * (b - a)
///
/// Java reference: `Mth.lerp(double, double, double)`
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn lerp<F>(alpha: F, a: F, b: F) -> F
where
    F: Copy + ops::Mul<Output = F> + ops::Add<Output = F> + ops::Sub<Output = F>,
{
    a + alpha * (b - a)
}

/// SIMD linear interpolation.
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn lerp_simd<F, const N: usize>(alpha: Simd<F, N>, a: Simd<F, N>, b: Simd<F, N>) -> Simd<F, N>
where
    F: SimdElement,
    Simd<F, N>: ops::Mul<Output = Simd<F, N>>
        + ops::Add<Output = Simd<F, N>>
        + ops::Sub<Output = Simd<F, N>>,
{
    a + alpha * (b - a)
}

/// Bilinear interpolation.
///
/// Interpolates between 4 values in a 2D grid.
///
/// Java reference: `Mth.lerp2(double, double, double, double, double, double)`
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn lerp2<F>(a1: F, a2: F, x00: F, x10: F, x01: F, x11: F) -> F
where
    F: Copy + ops::Mul<Output = F> + ops::Add<Output = F> + ops::Sub<Output = F>,
{
    lerp(a2, lerp(a1, x00, x10), lerp(a1, x01, x11))
}

/// SIMD bilinear interpolation.
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn lerp2_simd<F, const N: usize>(
    a1: Simd<F, N>,
    a2: Simd<F, N>,
    x00: Simd<F, N>,
    x10: Simd<F, N>,
    x01: Simd<F, N>,
    x11: Simd<F, N>,
) -> Simd<F, N>
where
    F: SimdElement,
    Simd<F, N>: ops::Mul<Output = Simd<F, N>>
        + ops::Add<Output = Simd<F, N>>
        + ops::Sub<Output = Simd<F, N>>,
{
    lerp_simd(a2, lerp_simd(a1, x00, x10), lerp_simd(a1, x01, x11))
}

/// Trilinear interpolation.
///
/// Interpolates between 8 values in a 3D grid.
///
/// Java reference: `Mth.lerp3(...)`
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "matches vanilla's Mth.lerp3 signature with 8 grid corner values"
)]
pub fn lerp3<F>(
    a1: F,
    a2: F,
    a3: F,
    x000: F,
    x100: F,
    x010: F,
    x110: F,
    x001: F,
    x101: F,
    x011: F,
    x111: F,
) -> F
where
    F: Copy + ops::Mul<Output = F> + ops::Add<Output = F> + ops::Sub<Output = F>,
{
    lerp(
        a3,
        lerp2(a1, a2, x000, x100, x010, x110),
        lerp2(a1, a2, x001, x101, x011, x111),
    )
}

/// Trilinear interpolation for N lanes. see lerp3.
#[inline]
#[expect(clippy::too_many_arguments, reason = "mirrors lerp3 with SIMD vectors")]
#[must_use]
pub fn lerp3_simd<F, const N: usize>(
    a1: Simd<F, N>,
    a2: Simd<F, N>,
    a3: Simd<F, N>,
    x000: Simd<F, N>,
    x100: Simd<F, N>,
    x010: Simd<F, N>,
    x110: Simd<F, N>,
    x001: Simd<F, N>,
    x101: Simd<F, N>,
    x011: Simd<F, N>,
    x111: Simd<F, N>,
) -> Simd<F, N>
where
    F: SimdElement,
    Simd<F, N>: ops::Mul<Output = Simd<F, N>>
        + ops::Add<Output = Simd<F, N>>
        + ops::Sub<Output = Simd<F, N>>,
{
    lerp_simd(
        a3,
        lerp2_simd(a1, a2, x000, x100, x010, x110),
        lerp2_simd(a1, a2, x001, x101, x011, x111),
    )
}

#[cfg(test)]
mod lerp_tests {
    use super::*;

    #[test]
    fn test_lerp() {
        assert!((lerp(0.0, 10.0, 20.0) - 10.0).abs() < 1e-10);
        assert!((lerp(1.0, 10.0, 20.0) - 20.0).abs() < 1e-10);
        assert!((lerp(0.5, 10.0, 20.0) - 15.0).abs() < 1e-10);
    }
}
/// Map a value from one range to another (unclamped).
///
/// Unlike [`map_clamped`], the result can extrapolate outside `[to_min, to_max]`.
///
/// Java reference: `Mth.map(double, double, double, double, double)`
#[inline]
#[must_use]
pub fn map(value: f64, from_min: f64, from_max: f64, to_min: f64, to_max: f64) -> f64 {
    lerp(inverse_lerp(value, from_min, from_max), to_min, to_max)
}

/// Map a value from one range to another with clamped lerp.
///
/// Used for Y-clamped gradients in density functions.
#[inline]
#[must_use]
pub fn map_clamped<T>(value: T, from_min: T, from_max: T, to_min: T, to_max: T) -> T
where
    T: ops::Add<Output = T>
        + Copy
        + ops::Div<Output = T>
        + From<f32>
        + ops::Mul<Output = T>
        + PartialOrd
        + ops::Sub<Output = T>,
{
    let t = (value - from_min) / (from_max - from_min);
    clamped_lerp(to_min, to_max, t)
}
/// Smoothstep - quintic Hermite interpolation (NOT cubic!)
///
/// Formula: 6x^5 - 15x^4 + 10x^3
///
/// This is the standard smoothstep used in Perlin noise for smooth transitions.
/// Java reference: `Mth.smoothstep(double)`
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn smoothstep<F>(x: F) -> F
where
    F: Copy + From<f32> + ops::Mul<Output = F> + ops::Add<Output = F> + ops::Sub<Output = F>,
{
    x * x * x * (x * (x * F::from(6.0) - F::from(15.0)) + F::from(10.0))
}

/// Smoothstep derivative for noise with derivatives.
///
/// Formula: 30x^2(x-1)^2
///
/// Java reference: `Mth.smoothstepDerivative(double)`
#[inline]
#[must_use]
pub fn smoothstep_derivative<F>(x: F) -> F
where
    F: Copy + From<f32> + ops::Mul<Output = F> + ops::Add<Output = F> + ops::Sub<Output = F>,
{
    F::from(30.0) * x * x * (x - F::from(1.0)) * (x - F::from(1.0))
}

/// Smoothstep for N lanes: 6x^5 - 15x^4 + 10x^3. Per-lane identical to [`smoothstep`].
#[inline]
#[must_use]
pub fn smoothstep_simd<F, const N: usize>(x: Simd<F, N>) -> Simd<F, N>
where
    F: SimdElement + SimdCast,
    Simd<F, N>: ops::Mul<Output = Simd<F, N>>
        + ops::Sub<Output = Simd<F, N>>
        + ops::Add<Output = Simd<F, N>>,
{
    x * x
        * x
        * (x * (x * Simd::splat(6.0).cast() - Simd::splat(15.0).cast()) + Simd::splat(10.0).cast())
}

#[cfg(test)]
mod smoothstep_tests {
    use super::*;
    #[test]
    fn test_smoothstep() {
        // At boundaries
        assert!((smoothstep(0.0) - 0.0).abs() < 1e-10);
        assert!((smoothstep(1.0) - 1.0).abs() < 1e-10);
        // At midpoint
        assert!((smoothstep(0.5) - 0.5).abs() < 1e-10);
    }
}
