//! Common angle constants.

use std::f32::consts::PI as PI_F32;
use std::f64::consts::PI as PI_F64;

/// Degrees in a quarter turn.
pub const DEGREE_90: f32 = 90.0;
/// Degrees in a half turn.
pub const DEGREE_180: f32 = 180.0;
/// Degrees in three quarters of a turn.
pub const DEGREE_270: f32 = 270.0;
/// Degrees in a full turn.
pub const DEGREE_360: f32 = 360.0;

/// Multiplier for converting `f32` degrees to radians.
///
/// This matches vanilla's `Mth.DEG_TO_RAD`.
pub const DEG_TO_RAD: f32 = PI_F32 / DEGREE_180;
/// Multiplier for converting `f32` radians to degrees.
///
/// This matches vanilla's `Mth.RAD_TO_DEG`.
pub const RAD_TO_DEG: f32 = DEGREE_180 / PI_F32;

/// Multiplier for converting `f64` degrees to radians.
pub const DEG_TO_RAD_F64: f64 = PI_F64 / DEGREE_180 as f64;
/// Multiplier for converting `f64` radians to degrees.
pub const RAD_TO_DEG_F64: f64 = DEGREE_180 as f64 / PI_F64;

/// Wraps an angle in degrees to vanilla's `[-180, 180)` range.
#[must_use]
pub fn wrap_degrees(mut degrees: f32) -> f32 {
    degrees %= DEGREE_360;
    if degrees >= DEGREE_180 {
        degrees -= DEGREE_360;
    }
    if degrees < -DEGREE_180 {
        degrees += DEGREE_360;
    }
    degrees
}

/// Converts an angle in degrees to vanilla's 16-segment rotation value.
#[must_use]
pub fn convert_to_rotation_segment(degrees: f32) -> u8 {
    (((degrees.rem_euclid(DEGREE_360) / (DEGREE_360 / 16.0)) + 0.5) as u8) & 15
}

#[cfg(test)]
mod tests {
    use super::{convert_to_rotation_segment, wrap_degrees};

    #[test]
    fn wrap_degrees_matches_vanilla_range() {
        assert_eq!(wrap_degrees(181.0).to_bits(), (-179.0_f32).to_bits());
        assert_eq!(wrap_degrees(-181.0).to_bits(), 179.0_f32.to_bits());
        assert_eq!(wrap_degrees(90.0).to_bits(), 90.0_f32.to_bits());
        assert_eq!(wrap_degrees(540.0).to_bits(), (-180.0_f32).to_bits());
    }

    #[test]
    fn rotation_segment_wraps_and_rounds_to_nearest_segment() {
        assert_eq!(convert_to_rotation_segment(11.24), 0);
        assert_eq!(convert_to_rotation_segment(11.25), 1);
        assert_eq!(convert_to_rotation_segment(-90.0), 12);
        assert_eq!(convert_to_rotation_segment(360.0), 0);
    }
}
