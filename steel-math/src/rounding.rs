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

#[cfg(test)]
mod tests {
    use super::round_f32;

    #[test]
    fn round_f32_matches_java_math_round_on_halves() {
        assert_eq!(round_f32(2.5), 3);
        assert_eq!(round_f32(-2.5), -2);
        assert_eq!(round_f32(2.4), 2);
        assert_eq!(round_f32(-2.6), -3);
        assert_eq!(round_f32(0.499_999_97), 0);
        assert_eq!(round_f32(f32::NAN), 0);
        assert_eq!(round_f32(1.0e10), i32::MAX);
    }
}
