//! Noise generation utilities matching vanilla Minecraft's noise system.
//!
//! This module provides the noise generation primitives used for world generation:
//!
//! - [`ImprovedNoise`] - Base Perlin noise implementation
//! - [`PerlinNoise`] - Octave-based Perlin noise
//! - [`NormalNoise`] - Double Perlin noise (used for biome climate parameters)
//! - [`SimplexNoise`] - Simplex noise (used for End island generation)

mod improved_noise;
mod normal_noise;
mod perlin_noise;
mod simplex_noise;

pub use improved_noise::ImprovedNoise;
pub use normal_noise::NormalNoise;
pub use perlin_noise::PerlinNoise;
pub use simplex_noise::SimplexNoise;

/// Gradient vectors shared between Perlin and simplex noise (from vanilla `SimplexNoise.GRADIENT`).
pub(crate) const GRADIENT: [[i32; 3]; 16] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
    [1, 1, 0],
    [0, -1, 1],
    [-1, 1, 0],
    [0, -1, -1],
];
