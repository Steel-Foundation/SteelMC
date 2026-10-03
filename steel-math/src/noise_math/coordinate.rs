use std::{
    ops,
    simd::{
        Mask, Select, Simd, SimdCast, SimdElement, StdFloat,
        cmp::{SimdPartialEq, SimdPartialOrd},
        num::{SimdFloat, SimdInt},
    },
};

/// Floor function that matches Java behavior.
///
/// In Java, `(int)v` truncates toward zero, but we need floor behavior.
/// For negative values, we need to subtract 1 if there's a fractional part.
///
/// Fast Floor from Stefan Gustavson's in "Simplex Noise Demystified" 2005 paper
///
/// Java reference: `Mth.floor(double)`
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn fast_floor(v: f64) -> i32 {
    let i = v as i32;
    if v < f64::from(i) { i - 1 } else { i }
}

/// SIMD implementation of `fast_floor`.
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn fast_floor_simd<F, I, const N: usize>(v: Simd<F, N>) -> Simd<I, N>
where
    F: SimdElement + SimdCast,
    I: SimdElement + SimdCast,
    Simd<F, N>: SimdFloat<Cast<I> = Simd<I, N>>
        + SimdPartialOrd
        + SimdPartialEq<Mask = Mask<<F as SimdElement>::Mask, N>>,
    Simd<I, N>: SimdInt<Cast<F> = Simd<F, N>> + ops::Sub<Output = Simd<I, N>>,
{
    let i = v.cast::<I>();
    let b = v.simd_lt(i.cast::<F>());
    b.select(i - Simd::splat(1).cast(), i)
}

/// Long floor function matching Java behavior.
///
/// Java reference: `Mth.lfloor(double)`
#[inline]
#[must_use]
pub fn fast_lfloor(v: f64) -> i64 {
    let i = v as i64;
    if v < i as f64 { i - 1 } else { i }
}

#[cfg(test)]
mod fast_floor_tests {
    use super::*;
    #[test]
    fn test_floor() {
        assert_eq!(fast_floor(1.5), 1);
        assert_eq!(fast_floor(1.0), 1);
        assert_eq!(fast_floor(0.5), 0);
        assert_eq!(fast_floor(0.0), 0);
        assert_eq!(fast_floor(-0.5), -1);
        assert_eq!(fast_floor(-1.0), -1);
        assert_eq!(fast_floor(-1.5), -2);
    }
}
/// Round-off constant for coordinate wrapping to prevent precision loss.
/// This is 2^25 = 33554432.
const ROUND_OFF: f64 = 33_554_432.0;
const HALF_ROUND_OFF: f64 = ROUND_OFF / 2.0;

/// Wrap N coordinates to prevent precision loss (N-lane SIMD version of [`wrap`]).
///
/// Fast path: at normal game coordinates all lanes are within `[-HALF_ROUND_OFF, HALF_ROUND_OFF)`,
/// so the expensive `div + floor + mul` is skipped almost always.
#[inline]
#[must_use]
pub fn wrap_simd<F, const N: usize>(x: Simd<F, N>) -> Simd<F, N>
where
    F: SimdElement + SimdCast,
    Simd<F, N>: ops::Div<Output = Simd<F, N>>
        + ops::Add<Output = Simd<F, N>>
        + ops::Mul<Output = Simd<F, N>>
        + ops::Sub<Output = Simd<F, N>>
        + SimdPartialOrd<Mask = Mask<<F as SimdElement>::Mask, N>>
        + StdFloat,
{
    let in_fast_range = x.simd_ge(Simd::splat(-HALF_ROUND_OFF).cast())
        & x.simd_lt(Simd::splat(HALF_ROUND_OFF).cast());
    if in_fast_range.all() {
        return x;
    }

    let round_off = Simd::splat(ROUND_OFF).cast();
    x - (x / round_off + Simd::splat(0.5).cast()).floor() * round_off
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
mod wrap_tests {
    use super::*;
    use std::simd::f64x4;
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
            let wrapped = wrap_simd(f64x4::from_array(case)).to_array();
            for (input, actual) in case.into_iter().zip(wrapped) {
                assert_eq!(actual, wrap(input));
            }
        }
    }
}
