use core::simd::f64x4;

use crate::GRADIENT;

/// Gather gradient components for 4 hashes into separate x/y/z SIMD vectors,
/// then compute the dot product with the given position vectors.
#[inline]
#[must_use]
pub fn grad_dot_4x(hashes: [usize; 4], x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
    let mut gx = [0.0f64; 4];
    let mut gy = [0.0f64; 4];
    let mut gz = [0.0f64; 4];
    for i in 0..4 {
        let g = &GRADIENT[hashes[i] & 15];
        gx[i] = g[0];
        gy[i] = g[1];
        gz[i] = g[2];
    }
    f64x4::from_array(gx) * x + f64x4::from_array(gy) * y + f64x4::from_array(gz) * z
}

/// Calculate the dot product of a gradient vector and the position vector.
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn grad_dot(hash: usize, x: f64, y: f64, z: f64) -> f64 {
    let g = &GRADIENT[hash & 15];
    g[0] * x + g[1] * y + g[2] * z
}
