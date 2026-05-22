/// Inverse linear interpolation (find the factor t such that lerp(t, a, b) == value).
///
/// Java reference: `Mth.inverseLerp(double, double, double)`
#[inline]
#[must_use]
pub fn inverse_lerp(value: f64, a: f64, b: f64) -> f64 {
    (value - a) / (b - a)
}
