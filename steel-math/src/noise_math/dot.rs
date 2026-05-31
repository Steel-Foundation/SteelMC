/// Dot product of gradient vector and offset vector.
#[inline]
#[must_use]
pub fn dot(g: &[f64; 3], x: f64, y: f64, z: f64) -> f64 {
    g[0] * x + g[1] * y + g[2] * z
}
