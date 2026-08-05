//! all the math of steel

#![feature(portable_simd)]
/// Math utilities used by vanilla world generation noise.
mod noise_math;
/// SIMD-based utility functions for matrix transpositions and vector manipulations.
#[cfg(not(target_feature = "avx512f"))]
mod simd_utils;
pub mod trig;

pub use crate::noise_math::*;
