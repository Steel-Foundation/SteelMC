use core::simd::cmp::{SimdPartialEq, SimdPartialOrd};
use core::simd::{Select, Simd, f64x4};

use crate::GRADIENT;

/// Gather gradient components for 4 hashes into separate x/y/z SIMD vectors,
/// then compute the dot product with the given position vectors.
#[inline]
#[must_use]
pub fn grad_dot_4x(hashes: [usize; 4], x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
    grad_dot_simd::<4>(hashes, x, y, z)
}

/// Generic N-lane gradient dot product.
///
/// Evaluates Minecraft's 16-entry `GRADIENT` table **branchlessly from the hash
/// bits** — Ken Perlin's reference `grad()`, which is value-identical to
/// indexing `GRADIENT[hash & 15]` for all 16 entries (verified per-entry). This
/// replaces the per-lane table gather + scalar→vector marshaling that dominated
/// the kernel (~70% of its instructions, profiled) with pure vector
/// compares/selects/negations — no memory gather, no lane assembly.
///
/// The earlier table forms (scalar build, and a `vgatherqpd` `SoA` variant) were
/// both bottlenecked on getting the gathered components into SIMD lanes; this
/// sidesteps that entirely.
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
