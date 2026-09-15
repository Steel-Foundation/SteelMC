use std::simd::Simd;
#[cfg(target_feature = "avx512f")]
use std::simd::{
    Select,
    cmp::{SimdPartialEq, SimdPartialOrd},
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

/// See `GRADIENT` for details. This is a f32 version of the gradient table for use in f32 noise functions
pub const GRADIENT_F32: [[f32; 3]; 16] = gradient_f32();

/// Dot product of gradient vector and offset vector.
#[inline]
#[must_use]
pub fn dot(g: &[f64; 3], x: f64, y: f64, z: f64) -> f64 {
    g[0] * x + g[1] * y + g[2] * z
}
/// f32 N-lane gradient dot product.
///
/// AVX-512 builds evaluate Minecraft's 16-entry `GRADIENT` table branchlessly
/// from the hash bits. Baseline builds assemble component vectors from the
/// table, which avoids expensive mask work on current non-AVX-512 targets.
#[inline]
#[must_use]
pub fn grad_dot_simd<const N: usize>(
    hashes: [usize; N],
    x: Simd<f32, N>,
    y: Simd<f32, N>,
    z: Simd<f32, N>,
) -> Simd<f32, N>
where
    Simd<f32, N>: std::ops::Mul<Output = Simd<f32, N>>
        + std::ops::Add<Output = Simd<f32, N>>
        + std::ops::Sub<Output = Simd<f32, N>>
        + std::ops::Neg<Output = Simd<f32, N>>,
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
        let gx = Simd::from_array(gradients.map(|gradient| gradient[0] as f32));
        let gy = Simd::from_array(gradients.map(|gradient| gradient[1] as f32));
        let gz = Simd::from_array(gradients.map(|gradient| gradient[2] as f32));
        gx * x + gy * y + gz * z
    }
}

/// Calculate the f32 dot product used by vanilla Perlin noise.
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn grad_dot(hash: usize, x: f32, y: f32, z: f32) -> f32 {
    let g = &GRADIENT_F32[hash & 15];
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

const fn gradient_f32() -> [[f32; 3]; 16] {
    let mut result = [[0.0; 3]; 16];
    let mut i = 0;

    while i < GRADIENT.len() {
        result[i] = [
            GRADIENT[i][0] as f32,
            GRADIENT[i][1] as f32,
            GRADIENT[i][2] as f32,
        ];
        i += 1;
    }

    result
}
