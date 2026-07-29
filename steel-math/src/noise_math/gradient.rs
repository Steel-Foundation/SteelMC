#[cfg(not(target_feature = "avx512f"))]
use crate::simd_utils::transpose;
#[cfg(not(target_feature = "avx512f"))]
use std::simd::num::SimdFloat;
#[cfg(target_feature = "avx512f")]
use std::simd::{
    Select,
    cmp::{SimdPartialEq, SimdPartialOrd},
};
use std::{
    ops,
    simd::{Simd, SimdCast, SimdElement},
};

/// Gradient vectors shared between Perlin and simplex noise (from vanilla `SimplexNoise.GRADIENT`).
pub const GRADIENT: [[f64; 3]; 16] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
    [1.0, 1.0, 0.0],
    [0.0, -1.0, 1.0],
    [-1.0, 1.0, 0.0],
    [0.0, -1.0, -1.0],
];

/// Same as Gradient but with a fourth 0 to be more simd friendly
pub const GRADIENT_4: [[f64; 4]; 16] = [
    [1.0, 1.0, 0.0, 0.],
    [-1.0, 1.0, 0.0, 0.],
    [1.0, -1.0, 0.0, 0.],
    [-1.0, -1.0, 0.0, 0.],
    [1.0, 0.0, 1.0, 0.],
    [-1.0, 0.0, 1.0, 0.],
    [1.0, 0.0, -1.0, 0.],
    [-1.0, 0.0, -1.0, 0.],
    [0.0, 1.0, 1.0, 0.],
    [0.0, -1.0, 1.0, 0.],
    [0.0, 1.0, -1.0, 0.],
    [0.0, -1.0, -1.0, 0.],
    [1.0, 1.0, 0.0, 0.],
    [0.0, -1.0, 1.0, 0.],
    [-1.0, 1.0, 0.0, 0.],
    [0.0, -1.0, -1.0, 0.],
];
/// Dot product of gradient vector and offset vector.
#[inline]
#[must_use]
pub fn dot(g: &[f64; 3], x: f64, y: f64, z: f64) -> f64 {
    g[0] * x + g[1] * y + g[2] * z
}
/// Calculate 4 gradient dot products.
///
/// Baseline builds use table assembly because it is faster without AVX-512
/// masks; native AVX-512 builds use the branchless hash formula.
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn grad_dot_4x<F>(hashes: [usize; 4], x: Simd<F, 4>, y: Simd<F, 4>, z: Simd<F, 4>) -> Simd<F, 4>
where
    F: SimdElement + SimdCast,
    Simd<F, 4>: ops::Mul<Output = Simd<F, 4>>
        + ops::Add<Output = Simd<F, 4>>
        + ops::Sub<Output = Simd<F, 4>>
        + ops::Neg<Output = Simd<F, 4>>,
{
    #[cfg(target_feature = "avx512f")]
    {
        grad_dot_simd(hashes, x, y, z)
    }

    #[cfg(not(target_feature = "avx512f"))]
    {
        let h0 = Simd::from_array(GRADIENT_4[hashes[0] & 15]).cast::<F>();
        let h1 = Simd::from_array(GRADIENT_4[hashes[1] & 15]).cast::<F>();
        let h2 = Simd::from_array(GRADIENT_4[hashes[2] & 15]).cast::<F>();
        let h3 = Simd::from_array(GRADIENT_4[hashes[3] & 15]).cast::<F>();

        let (gx, gy, gz, _gw) = transpose(h0, h1, h2, h3);

        gx * x + gy * y + gz * z
    }
}

/// Generic N-lane gradient dot product.
///
/// AVX-512 builds evaluate Minecraft's 16-entry `GRADIENT` table branchlessly
/// from the hash bits. Baseline builds assemble component vectors from the
/// table, which avoids expensive mask work on current non-AVX-512 targets.
#[inline]
#[must_use]
pub fn grad_dot_simd<F, const N: usize>(
    hashes: [usize; N],
    x: Simd<F, N>,
    y: Simd<F, N>,
    z: Simd<F, N>,
) -> Simd<F, N>
where
    F: SimdElement + SimdCast,
    Simd<F, N>: ops::Mul<Output = Simd<F, N>>
        + ops::Add<Output = Simd<F, N>>
        + ops::Sub<Output = Simd<F, N>>
        + ops::Neg<Output = Simd<F, N>>,
{
    #[cfg(target_feature = "avx512f")]
    {
        let hash_lanes = Simd::<i64, N>::from_array(hashes.map(|value| (value & 15) as i64));
        let u_component = hash_lanes.simd_lt(Simd::splat(8)).select(x, y);
        let v_component = hash_lanes.simd_lt(Simd::splat(4)).select(
            y,
            (hash_lanes.simd_eq(Simd::splat(12)) | hash_lanes.simd_eq(Simd::splat(14)))
                .select(x, z),
        );
        let signed_u = (hash_lanes & Simd::splat(1))
            .simd_eq(Simd::splat(0))
            .select(u_component, -u_component);
        let signed_v = (hash_lanes & Simd::splat(2))
            .simd_eq(Simd::splat(0))
            .select(v_component, -v_component);
        signed_u + signed_v
    }

    #[cfg(not(target_feature = "avx512f"))]
    {
        let gradients = hashes.map(|hash| GRADIENT[hash & 15]);
        let gx = Simd::from_array(gradients.map(|gradient| gradient[0])).cast::<F>();
        let gy = Simd::from_array(gradients.map(|gradient| gradient[1])).cast::<F>();
        let gz = Simd::from_array(gradients.map(|gradient| gradient[2])).cast::<F>();
        gx * x + gy * y + gz * z
    }
}

/// Calculate the dot product of a gradient vector and the position vector.
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn grad_dot(hash: usize, x: f64, y: f64, z: f64) -> f64 {
    let g = &GRADIENT[hash & 15];
    g[0] * x + g[1] * y + g[2] * z
}
/// Compute corner noise contribution for a simplex vertex.
#[inline]
#[must_use]
pub fn corner_noise_3d(index: usize, x: f64, y: f64, z: f64, base: f64) -> f64 {
    let t0 = base - x * x - y * y - z * z;
    if t0 < 0.0 {
        0.0
    } else {
        let t0 = t0 * t0;
        t0 * t0 * dot(&GRADIENT[index], x, y, z)
    }
}
