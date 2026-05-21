use core::f64::consts::PI;

/// Bias a noise value towards extremes (-1 or 1) using a sine curve.
///
/// Java reference: `NoiseUtils.biasTowardsExtreme(double, double)`
#[inline]
#[must_use]
pub fn bias_towards_extreme(noise: f64, factor: f64) -> f64 {
    noise + (PI * noise).sin() * factor / PI
}
