mod coordinate;
mod gradient;
mod interpolation;

use core::f64::consts::PI;

pub use coordinate::{fast_floor, fast_floor_simd, fast_lfloor, wrap, wrap_simd};
pub use gradient::{
    GRADIENT, GRADIENT_4, corner_noise_3d, dot, grad_dot, grad_dot_4x, grad_dot_simd,
};
pub use interpolation::{
    clamp, clamp_i32, clamped_lerp, clamped_lerp_simd, inverse_lerp, lerp, lerp_simd, lerp2,
    lerp2_simd, lerp3, lerp3_simd, map, map_clamped, smoothstep, smoothstep_derivative,
    smoothstep_simd,
};

/// Bias a noise value towards extremes (-1 or 1) using a sine curve.
///
/// Java reference: `NoiseUtils.biasTowardsExtreme(double, double)`
#[inline]
#[must_use]
pub fn bias_towards_extreme(noise: f64, factor: f64) -> f64 {
    noise + (PI * noise).sin() * factor / PI
}

/// Cube a value.
#[inline]
#[must_use]
pub fn cube(x: f64) -> f64 {
    x * x * x
}

/// Square a value.
#[inline]
#[must_use]
pub fn square(x: f64) -> f64 {
    x * x
}
