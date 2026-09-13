//! all the math of steel

#![feature(portable_simd)]
/// Shared angle constants.
pub mod angle;
/// Math utilities used by vanilla world generation noise.
mod noise_math;
/// SIMD-based utility functions for matrix transpositions and vector manipulations.
#[cfg(not(target_feature = "avx512f"))]
mod simd_utils;
pub mod trig;

pub use crate::angle::{
    DEG_TO_RAD, DEG_TO_RAD_F64, DEGREE_90, DEGREE_180, DEGREE_270, DEGREE_360, RAD_TO_DEG,
    RAD_TO_DEG_F64, convert_to_rotation_segment, wrap_degrees,
};
pub use crate::noise_math::*;
