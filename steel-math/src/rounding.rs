//! Integer rounding compatible with Java's `Math.round` overloads.

/// Rounds to the nearest integer, with ties toward positive infinity.
///
/// Returns zero for NaN and saturates at the `i32` bounds, matching
/// Java's `Math.round(float)`.
#[inline]
#[must_use]
pub fn java_round_f32(value: f32) -> i32 {
    // Widen so the value directly below 0.5 does not round up during addition.
    (f64::from(value) + 0.5).floor() as i32
}

/// Rounds to the nearest integer, with ties toward positive infinity.
///
/// Returns zero for NaN and saturates at the `i64` bounds, matching
/// Java's `Math.round(double)`.
#[inline]
#[must_use]
pub const fn java_round_f64(value: f64) -> i64 {
    let floor = value.floor();
    // Adding 0.5 directly can round the value just below a tie up to that tie.
    if value - floor >= 0.5 {
        (floor + 1.0) as i64
    } else {
        floor as i64
    }
}

#[cfg(test)]
mod tests {
    use super::{java_round_f32, java_round_f64};

    #[test]
    fn float_rounding_matches_java_at_ties_precision_limits_and_nonfinite_values() {
        for (value, expected) in [
            (f32::from_bits(0x3eff_ffff), 0),
            (0.5, 1),
            (-0.5, 0),
            ((-0.5_f32).next_down(), -1),
            (-1.5, -1),
            (2.3, 2),
            (-2.3, -2),
            (8_388_609.0, 8_388_609),
            (i32::MAX as f32, i32::MAX),
            (i32::MIN as f32, i32::MIN),
            (f32::NAN, 0),
            (f32::INFINITY, i32::MAX),
            (f32::NEG_INFINITY, i32::MIN),
        ] {
            assert_eq!(java_round_f32(value), expected, "value={value:?}");
        }
    }

    #[test]
    fn double_rounding_matches_java_at_ties_precision_limits_and_nonfinite_values() {
        for (value, expected) in [
            (0.5_f64.next_down(), 0),
            (0.5, 1),
            (-0.5, 0),
            ((-0.5_f64).next_down(), -1),
            (-1.5, -1),
            (2.3, 2),
            (-2.3, -2),
            (4_503_599_627_370_497.0, 4_503_599_627_370_497),
            (i64::MAX as f64, i64::MAX),
            (i64::MIN as f64, i64::MIN),
            (f64::NAN, 0),
            (f64::INFINITY, i64::MAX),
            (f64::NEG_INFINITY, i64::MIN),
        ] {
            assert_eq!(java_round_f64(value), expected, "value={value:?}");
        }
    }
}
