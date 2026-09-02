//! # Angle utilities

/// Wraps an angle in degrees to the range [-180, 180)
#[must_use]
pub fn wrap_degrees(mut degrees: f32) -> f32 {
    degrees %= 360.0;
    if degrees >= 180.0 {
        degrees -= 360.0;
    }
    if degrees < -180.0 {
        degrees += 360.0;
    }
    degrees
}

/// Converts a rotation in degrees to vanilla 16 segment rotation value
#[must_use]
pub fn convert_to_rotation_segment(degrees: f32) -> u8 {
    (((degrees.rem_euclid(360.0) / 22.5) + 0.5) as u8) & 15
}

#[cfg(test)]
mod tests {
    use super::wrap_degrees;

    #[test]
    fn wrap_degrees_matches_vanilla_range() {
        assert_eq!(wrap_degrees(181.0).to_bits(), (-179.0_f32).to_bits());
        assert_eq!(wrap_degrees(-181.0).to_bits(), 179.0_f32.to_bits());
        assert_eq!(wrap_degrees(90.0).to_bits(), 90.0_f32.to_bits());
        assert_eq!(wrap_degrees(540.0).to_bits(), (-180.0_f32).to_bits());
    }
}
