use core::simd::f64x4;
use std::simd::StdFloat;

/// Round-off constant for coordinate wrapping to prevent precision loss.
/// This is 2^25 = 33554432.
const ROUND_OFF: f64 = 33_554_432.0;
const HALF_ROUND_OFF: f64 = ROUND_OFF / 2.0;

/// Wrap 4 coordinates to prevent precision loss (SIMD version of [`wrap`]).
#[inline]
#[must_use]
pub fn wrap_4x(x: f64x4) -> f64x4 {
    let round_off = f64x4::splat(ROUND_OFF);
    x - (x / round_off + f64x4::splat(0.5)).floor() * round_off
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
}
