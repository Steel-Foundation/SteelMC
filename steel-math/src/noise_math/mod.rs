mod bias_towards_extreme;
mod clamp;
mod corner_noise_3d;
mod cube;
mod dot;
mod fast_floor;
mod grad_dot;
mod inverse_lerp;
mod lerp;
mod map;
mod smoothstep;
mod square;
mod wrap;

pub use bias_towards_extreme::bias_towards_extreme;
pub use clamp::{clamp, clamp_i32, clamped_lerp, clamped_lerp_simd};
pub use corner_noise_3d::corner_noise_3d;
pub use cube::cube;
pub use dot::dot;
pub use fast_floor::{fast_floor, fast_floor_simd, fast_lfloor};
pub use grad_dot::{grad_dot, grad_dot_4x, grad_dot_simd};
pub use inverse_lerp::inverse_lerp;
pub use lerp::{lerp, lerp_simd, lerp2, lerp2_simd, lerp3, lerp3_simd};
pub use map::{map, map_clamped};
pub use smoothstep::{smoothstep, smoothstep_derivative, smoothstep_simd};
pub use square::square;
pub use wrap::{wrap, wrap_simd};

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
