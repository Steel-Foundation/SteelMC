use crate::noise_math::{clamp::clamped_lerp, inverse_lerp::inverse_lerp, lerp::lerp};

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
pub fn map_clamped(value: f64, from_min: f64, from_max: f64, to_min: f64, to_max: f64) -> f64 {
    let t = (value - from_min) / (from_max - from_min);
    clamped_lerp(to_min, to_max, t)
}
