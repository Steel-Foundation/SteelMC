use core::simd::{Simd, f64x4};
use std::simd::StdFloat;
use std::simd::cmp::SimdPartialOrd;

/// Round-off constant for coordinate wrapping to prevent precision loss.
/// This is 2^25 = 33554432.
const ROUND_OFF: f64 = 33_554_432.0;
const HALF_ROUND_OFF: f64 = ROUND_OFF / 2.0;

/// Wrap 4 coordinates to prevent precision loss (SIMD version of [`wrap`]).
#[inline]
#[must_use]
pub fn wrap_4x(x: f64x4) -> f64x4 {
    wrap_simd::<4>(x)
}

/// Wrap N coordinates to prevent precision loss (N-lane SIMD version of [`wrap`]).
#[inline]
#[must_use]
pub fn wrap_simd<const N: usize>(x: Simd<f64, N>) -> Simd<f64, N> {
    let in_fast_range =
        x.simd_ge(Simd::splat(-HALF_ROUND_OFF)) & x.simd_lt(Simd::splat(HALF_ROUND_OFF));
    if in_fast_range.all() {
        return x;
    }

    let round_off = Simd::splat(ROUND_OFF);
    x - (x / round_off + Simd::splat(0.5)).floor() * round_off
}

/// Wrap a coordinate to prevent precision loss at large values.
///
/// This wraps the coordinate to the range `[-ROUND_OFF/2, ROUND_OFF/2]` to
/// maintain numerical precision for coordinates far from the origin.
///
/// Public because `BlendedNoise` calls this directly on per-octave coordinates.
#[inline]
#[must_use]
pub fn wrap(x: f64) -> f64 {
    if (-HALF_ROUND_OFF..HALF_ROUND_OFF).contains(&x) {
        return x;
    }

    x - (x / ROUND_OFF + 0.5).floor() * ROUND_OFF
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_wrap() {
        fn wrap_reference(x: f64) -> f64 {
            x - (x / ROUND_OFF + 0.5).floor() * ROUND_OFF
        }

        // Small values should be unchanged
        assert!((wrap(100.0) - 100.0).abs() < 1e-10);
        assert!((wrap(-100.0) - (-100.0)).abs() < 1e-10);

        // Very large values should be wrapped
        let large = 100_000_000.0;
        let wrapped = wrap(large);
        assert!(wrapped.abs() < ROUND_OFF);

        for x in [
            -HALF_ROUND_OFF,
            -HALF_ROUND_OFF + 1.0,
            0.0,
            HALF_ROUND_OFF - 1.0,
            HALF_ROUND_OFF,
            ROUND_OFF,
            -ROUND_OFF,
            100_000_000.0,
            -100_000_000.0,
        ] {
            assert!((wrap(x) - wrap_reference(x)).abs() < 1e-15);
        }
    }

    #[test]
    fn test_wrap_4x_matches_scalar_wrap() {
        let cases = [
            [0.0, 1.0, -1.0, HALF_ROUND_OFF - 1.0],
            [
                -HALF_ROUND_OFF,
                -HALF_ROUND_OFF + 1.0,
                HALF_ROUND_OFF - 1.0,
                HALF_ROUND_OFF,
            ],
            [ROUND_OFF, -ROUND_OFF, 100_000_000.0, -100_000_000.0],
            [1.25, HALF_ROUND_OFF, -20.5, -HALF_ROUND_OFF],
        ];

        for case in cases {
            let wrapped = wrap_4x(f64x4::from_array(case)).to_array();
            for (input, actual) in case.into_iter().zip(wrapped) {
                #[expect(
                    clippy::float_cmp,
                    reason = "SIMD wrap must be bit-identical to scalar wrap per lane"
                )]
                {
                    assert_eq!(actual, wrap(input));
                }
            }
        }
    }
}
