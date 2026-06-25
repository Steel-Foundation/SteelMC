use std::{
    ops,
    simd::{
        Mask, Select, Simd, SimdCast, SimdElement,
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
mod tests {
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
