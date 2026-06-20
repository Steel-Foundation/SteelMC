use core::simd::cmp::{SimdPartialEq, SimdPartialOrd};
use core::simd::{Select, Simd, f64x4};

use crate::GRADIENT;

/// Calculate 4 gradient dot products.
///
/// Baseline builds use table assembly because it is faster without AVX-512
/// masks; native AVX-512 builds use the branchless hash formula.
#[inline]
#[must_use]
pub fn grad_dot_4x(hashes: [usize; 4], x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
    #[cfg(target_feature = "avx512f")]
    {
        grad_dot_simd::<4>(hashes, x, y, z)
    }

    #[cfg(not(target_feature = "avx512f"))]
    {
        let mut gx = [0.0f64; 4];
        let mut gy = [0.0f64; 4];
        let mut gz = [0.0f64; 4];
        for i in 0..4 {
            let g = &GRADIENT[hashes[i] & 15];
            gx[i] = g[0];
            gy[i] = g[1];
            gz[i] = g[2];
        }
        f64x4::from_array(gx) * x + f64x4::from_array(gy) * y + f64x4::from_array(gz) * z
    }
}

/// Generic N-lane gradient dot product.
///
/// Evaluates Minecraft's 16-entry `GRADIENT` table branchlessly from the hash
/// bits using the public-domain reference formula from Ken Perlin's improved
/// noise implementation. For `hash & 15`, that formula is value-identical to
/// indexing Minecraft's `GRADIENT` table for all 16 entries.
///
/// Wider SIMD uses this path because the branchless formula avoids per-lane
/// component assembly.
#[inline]
#[must_use]
pub fn grad_dot_simd<const N: usize>(
    hashes: [usize; N],
    x: Simd<f64, N>,
    y: Simd<f64, N>,
    z: Simd<f64, N>,
) -> Simd<f64, N> {
    let hash_lanes = Simd::<i64, N>::from_array(hashes.map(|value| (value & 15) as i64));
    // u = h < 8 ? x : y
    let u_component = hash_lanes.simd_lt(Simd::splat(8)).select(x, y);
    // v = h < 4 ? y : (h == 12 || h == 14 ? x : z)
    let v_component = hash_lanes.simd_lt(Simd::splat(4)).select(
        y,
        (hash_lanes.simd_eq(Simd::splat(12)) | hash_lanes.simd_eq(Simd::splat(14))).select(x, z),
    );
    // grad·pos = ((h & 1) == 0 ? u : -u) + ((h & 2) == 0 ? v : -v)
    let signed_u = (hash_lanes & Simd::splat(1))
        .simd_eq(Simd::splat(0))
        .select(u_component, -u_component);
    let signed_v = (hash_lanes & Simd::splat(2))
        .simd_eq(Simd::splat(0))
        .select(v_component, -v_component);
    signed_u + signed_v
}

/// Calculate the dot product of a gradient vector and the position vector.
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn grad_dot(hash: usize, x: f64, y: f64, z: f64) -> f64 {
    let g = &GRADIENT[hash & 15];
    g[0] * x + g[1] * y + g[2] * z
}
