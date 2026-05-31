//! Noise generation utilities matching vanilla Minecraft's noise system.
//!
//! This module provides the noise generation primitives used for world generation:
//!
//! - [`ImprovedNoise`] - Base Perlin noise implementation
//! - [`PerlinNoise`] - Octave-based Perlin noise
//! - [`NormalNoise`] - Double Perlin noise (used for biome climate parameters)
//! - [`SimplexNoise`] - Simplex noise (used for End island generation)

mod aquifer;
mod beardifier;
mod blended_noise;
mod end_islands;
mod improved_noise;
mod noise_chunk;
mod normal_noise;
mod ore_veinifier;
mod perlin_noise;
mod perlin_simplex_noise;
mod simplex_noise;

pub use aquifer::{Aquifer, AquiferResult, LazyAquifer, preliminary_surface_level};
pub use beardifier::Beardifier;
pub use blended_noise::BlendedNoise;
pub use end_islands::EndIslands;
pub use improved_noise::ImprovedNoise;
pub use noise_chunk::NoiseChunk;
pub use normal_noise::NormalNoise;
pub use ore_veinifier::OreVeinifier;
pub use perlin_noise::PerlinNoise;
pub use perlin_simplex_noise::PerlinSimplexNoise;
pub use simplex_noise::SimplexNoise;
