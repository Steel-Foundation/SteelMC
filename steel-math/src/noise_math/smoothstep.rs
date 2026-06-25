use core::simd::Simd;
use std::{
    ops,
    simd::{SimdCast, SimdElement, num::SimdFloat},
};

/// Smoothstep - quintic Hermite interpolation (NOT cubic!)
///
/// Formula: 6x^5 - 15x^4 + 10x^3
///
/// This is the standard smoothstep used in Perlin noise for smooth transitions.
/// Java reference: `Mth.smoothstep(double)`
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn smoothstep(x: f64) -> f64 {
    x * x * x * (x * (x * 6.0 - 15.0) + 10.0)
}

/// Smoothstep derivative for noise with derivatives.
///
/// Formula: 30x^2(x-1)^2
///
/// Java reference: `Mth.smoothstepDerivative(double)`
#[inline]
#[must_use]
pub fn smoothstep_derivative(x: f64) -> f64 {
    30.0 * x * x * (x - 1.0) * (x - 1.0)
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
mod tests {
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
