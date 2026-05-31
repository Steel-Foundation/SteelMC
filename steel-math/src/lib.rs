//! all the math of steel

#![feature(portable_simd)]
/// Math utilities used by vanilla world generation noise.
mod noise_math;
pub mod trig;

pub use crate::noise_math::*;
