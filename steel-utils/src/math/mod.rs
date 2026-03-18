//! This module contains math related utilities.
/// An axis implementation
pub mod axis;
pub mod noise_math;

pub use axis::Axis;
pub use noise_math::{
    bias_towards_extreme, clamp, clamped_lerp, cube, floor, inverse_lerp, lerp, lerp2, lerp3,
    lfloor, map, map_clamped, smoothstep, smoothstep_derivative, square,
};
