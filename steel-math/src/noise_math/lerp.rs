use core::simd::{Simd, f64x4};

/// Linear interpolation.
///
/// Formula: a + alpha * (b - a)
///
/// Java reference: `Mth.lerp(double, double, double)`
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn lerp(alpha: f64, a: f64, b: f64) -> f64 {
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
pub fn lerp2(a1: f64, a2: f64, x00: f64, x10: f64, x01: f64, x11: f64) -> f64 {
    lerp(a2, lerp(a1, x00, x10), lerp(a1, x01, x11))
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
pub fn lerp3(
    a1: f64,
    a2: f64,
    a3: f64,
    x000: f64,
    x100: f64,
    x010: f64,
    x110: f64,
    x001: f64,
    x101: f64,
    x011: f64,
    x111: f64,
) -> f64 {
    lerp(
        a3,
        lerp2(a1, a2, x000, x100, x010, x110),
        lerp2(a1, a2, x001, x101, x011, x111),
    )
}

/// Linear interpolation for 4 lanes. see lerp.
#[inline]
#[must_use]
pub fn lerp_4x(alpha: f64x4, a: f64x4, b: f64x4) -> f64x4 {
    lerp_simd::<4>(alpha, a, b)
}

/// Bilinear interpolation for 4 lanes. see lerp2.
#[inline]
#[must_use]
pub fn lerp2_4x(a1: f64x4, a2: f64x4, x00: f64x4, x10: f64x4, x01: f64x4, x11: f64x4) -> f64x4 {
    lerp2_simd::<4>(a1, a2, x00, x10, x01, x11)
}

/// Trilinear interpolation for 4 lanes. see lerp3.
#[inline]
#[expect(clippy::too_many_arguments, reason = "mirrors lerp3 with SIMD vectors")]
#[must_use]
pub fn lerp3_4x(
    a1: f64x4,
    a2: f64x4,
    a3: f64x4,
    x000: f64x4,
    x100: f64x4,
    x010: f64x4,
    x110: f64x4,
    x001: f64x4,
    x101: f64x4,
    x011: f64x4,
    x111: f64x4,
) -> f64x4 {
    lerp3_simd::<4>(a1, a2, a3, x000, x100, x010, x110, x001, x101, x011, x111)
}

/// Linear interpolation for N lanes. see lerp.
#[inline]
#[must_use]
pub fn lerp_simd<const N: usize>(
    alpha: Simd<f64, N>,
    a: Simd<f64, N>,
    b: Simd<f64, N>,
) -> Simd<f64, N> {
    a + alpha * (b - a)
}

/// Bilinear interpolation for N lanes. see lerp2.
#[inline]
#[must_use]
pub fn lerp2_simd<const N: usize>(
    a1: Simd<f64, N>,
    a2: Simd<f64, N>,
    x00: Simd<f64, N>,
    x10: Simd<f64, N>,
    x01: Simd<f64, N>,
    x11: Simd<f64, N>,
) -> Simd<f64, N> {
    lerp_simd::<N>(
        a2,
        lerp_simd::<N>(a1, x00, x10),
        lerp_simd::<N>(a1, x01, x11),
    )
}

/// Trilinear interpolation for N lanes. see lerp3.
#[inline]
#[expect(clippy::too_many_arguments, reason = "mirrors lerp3 with SIMD vectors")]
#[must_use]
pub fn lerp3_simd<const N: usize>(
    a1: Simd<f64, N>,
    a2: Simd<f64, N>,
    a3: Simd<f64, N>,
    x000: Simd<f64, N>,
    x100: Simd<f64, N>,
    x010: Simd<f64, N>,
    x110: Simd<f64, N>,
    x001: Simd<f64, N>,
    x101: Simd<f64, N>,
    x011: Simd<f64, N>,
    x111: Simd<f64, N>,
) -> Simd<f64, N> {
    lerp_simd::<N>(
        a3,
        lerp2_simd::<N>(a1, a2, x000, x100, x010, x110),
        lerp2_simd::<N>(a1, a2, x001, x101, x011, x111),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_lerp() {
        assert!((lerp(0.0, 10.0, 20.0) - 10.0).abs() < 1e-10);
        assert!((lerp(1.0, 10.0, 20.0) - 20.0).abs() < 1e-10);
        assert!((lerp(0.5, 10.0, 20.0) - 15.0).abs() < 1e-10);
    }
}
