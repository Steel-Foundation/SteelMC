//! Java-compatible floating-point rounding

use crate::float_consts::{EXP_BIAS, EXP_BIT_MASK, SIGNIF_BIT_MASK, SIGNIFICAND_WIDTH, SIZE};

/// Rounds ties toward positive infinity, unlike [`f32::round`], which rounds
/// them away from zero. Uses the JDK's IEEE 754 bit-shift implementation of
/// [`Math.round(float)`], including saturation and NaN handling, without rounding
/// up values just below a half-integer
///
/// [`Math.round(float)`]: https://github.com/openjdk/jdk25u/blob/master/src/java.base/share/classes/java/lang/Math.java#L752-L783
#[must_use]
pub const fn round_f32(value: f32) -> i32 {
    let bits = value.to_bits() as i32;
    let biased_exp = (bits & EXP_BIT_MASK) >> (SIGNIFICAND_WIDTH - 1);
    let shift = (SIGNIFICAND_WIDTH - 2 + EXP_BIAS) - biased_exp;
    // The JDK spells this `(shift & -32) == 0`: a finite value whose ulp lies in [2^-32, 1).
    if shift >= 0 && shift < SIZE {
        let mut r = (bits & SIGNIF_BIT_MASK) | (SIGNIF_BIT_MASK + 1);
        if bits < 0 {
            r = -r;
        }
        ((r >> shift) + 1) >> 1
    } else {
        value as i32
    }
}

/// Rounds to the nearest integer, with ties toward positive infinity.
///
/// Returns zero for NaN and saturates at the `i64` bounds, matching
/// Java's `Math.round(double)`.
#[inline]
#[must_use]
pub const fn round_f64(value: f64) -> i64 {
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
    use super::{round_f32, round_f64};

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
            assert_eq!(round_f32(value), expected, "value={value:?}");
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
            assert_eq!(round_f64(value), expected, "value={value:?}");
        }
    }
}
