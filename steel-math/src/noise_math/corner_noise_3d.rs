use crate::{GRADIENT, noise_math::dot::dot};

/// Compute corner noise contribution for a simplex vertex.
#[inline]
#[must_use]
pub fn corner_noise_3d(index: usize, x: f64, y: f64, z: f64, base: f64) -> f64 {
    let t0 = base - x * x - y * y - z * z;
    if t0 < 0.0 {
        0.0
    } else {
        let t0 = t0 * t0;
        t0 * t0 * dot(&GRADIENT[index], x, y, z)
    }
}
