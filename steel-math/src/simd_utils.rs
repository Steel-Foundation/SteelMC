use std::simd::{Simd, SimdElement};

/// Transposes a 4x4 matrix of 64-bit floats represented by four SIMD vectors (`f64x4`).
#[inline]
#[must_use]
pub fn transpose<F>(
    r0: Simd<F, 4>, // [a0, a1, a2, a3]
    r1: Simd<F, 4>, // [b0, b1, b2, b3]
    r2: Simd<F, 4>, // [c0, c1, c2, c3]
    r3: Simd<F, 4>, // [d0, d1, d2, d3]
) -> (Simd<F, 4>, Simd<F, 4>, Simd<F, 4>, Simd<F, 4>)
where
    F: SimdElement,
{
    let (t0, t1) = r0.deinterleave(r1); // t0 = [a0, a2, b0, b2], t1 = [a1, a3, b1, b3]
    let (t2, t3) = r2.deinterleave(r3); // t2 = [c0, c2, d0, d2], t3 = [c1, c3, d1, d3]

    let (col0, col2) = t0.deinterleave(t2); // col0 = [a0, b0, c0, d0], col2 = [a2, b2, c2, d2]
    let (col1, col3) = t1.deinterleave(t3); // col1 = [a1, b1, c1, d1], col3 = [a3, b3, c3, d3]

    (col0, col1, col2, col3)
}
