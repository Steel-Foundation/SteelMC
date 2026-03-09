//! This module contains math related utilities.
pub mod noise_math;
pub mod vector2;
pub mod vector3;

pub use noise_math::{
    clamp, clamped_lerp, cube, floor, lerp, lerp2, lerp3, lfloor, map_clamped, smoothstep,
    smoothstep_derivative, square,
};
pub use vector2::Vector2;
pub use vector3::{Axis, Vector3};
