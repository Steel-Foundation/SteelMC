use core::simd::{Select, Simd, cmp::SimdPartialOrd};

use crate::noise_math::lerp::lerp;

/// Clamped linear interpolation.
///
/// Clamps the interpolation factor to [0, 1] before interpolating.
///
/// Java reference: `Mth.clampedLerp(double, double, double)`.
/// Note: Vanilla's parameter order is `(factor, min, max)`, ours is `(min, max, factor)`.
#[inline]
#[must_use]
pub fn clamped_lerp(min: f64, max: f64, factor: f64) -> f64 {
    if factor < 0.0 {
        min
    } else if factor > 1.0 {
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
