//! Packets for entity movement updates.
//!
//! These packets use fixed-point encoding for position deltas. The client maintains
//! a `VecDeltaCodec` for each entity which tracks the "base" position. Deltas are
//! computed as `encode(current) - encode(base)` where encode multiplies by 4096
//! and rounds.
//!
//! The server must track what the client's base position is (`PositionCodec`) to
//! compute correct deltas and know when the delta would overflow i16 bounds.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::{C_MOVE_ENTITY_POS, C_MOVE_ENTITY_POS_ROT, C_MOVE_ENTITY_ROT};

/// Fixed-point encoding multiplier (1/4096 block precision).
const TRUNCATION_STEPS: f64 = 4096.0;

/// Maximum delta value that fits in i16.
const MAX_DELTA: i64 = i16::MAX as i64;

/// Minimum delta value that fits in i16.
const MIN_DELTA: i64 = i16::MIN as i64;

/// Updates an entity's position with a delta from its current position.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_MOVE_ENTITY_POS)]
pub struct CMoveEntityPos {
    #[write(as = VarInt)]
    pub entity_id: i32,
    /// Delta X (current X * 4096 - previous X * 4096)
    pub dx: i16,
    /// Delta Y
    pub dy: i16,
    /// Delta Z
    pub dz: i16,
    pub on_ground: bool,
}

/// Updates an entity's position and rotation.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_MOVE_ENTITY_POS_ROT)]
pub struct CMoveEntityPosRot {
    #[write(as = VarInt)]
    pub entity_id: i32,
    /// Delta X (current X * 4096 - previous X * 4096)
    pub dx: i16,
    /// Delta Y
    pub dy: i16,
    /// Delta Z
    pub dz: i16,
    /// Yaw as angle byte
    pub y_rot: i8,
    /// Pitch as angle byte
    pub x_rot: i8,
    pub on_ground: bool,
}

/// Updates an entity's rotation only.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_MOVE_ENTITY_ROT)]
pub struct CMoveEntityRot {
    #[write(as = VarInt)]
    pub entity_id: i32,
    /// Yaw as angle byte
    pub y_rot: i8,
    /// Pitch as angle byte
    pub x_rot: i8,
    pub on_ground: bool,
}

/// Converts degrees to a protocol angle byte (0-255 representing 0-360 degrees).
///
/// Mirrors vanilla's `Mth.packDegrees()`: `(byte)floor(angle * 256.0F / 360.0F)`
#[inline]
#[must_use]
pub fn to_angle_byte(degrees: f32) -> i8 {
    // Vanilla: (byte)floor(angle * 256.0F / 360.0F)
    // Cast to i32 first (safe for all angle values), then truncate to i8.
    // This matches Java's (byte) cast which truncates the low 8 bits.
    (degrees * 256.0 / 360.0).floor() as i32 as i8
}

/// Encodes a position component to the protocol's fixed-point format.
///
/// Mirrors vanilla's `VecDeltaCodec.encode()` which uses `Math.round()`.
/// Java's `Math.round()` rounds half towards positive infinity (half-up),
/// which differs from Rust's `round()` that rounds half away from zero.
#[inline]
#[must_use]
pub fn encode_position(value: f64) -> i64 {
    // Java Math.round() rounds half towards positive infinity:
    //   Math.round(0.5) = 1, Math.round(-0.5) = 0
    // Rust round() rounds half away from zero:
    //   (0.5).round() = 1, (-0.5).round() = -1
    // To match Java, use floor(x + 0.5) which always rounds half-up.
    (value * TRUNCATION_STEPS + 0.5).floor() as i64
}

/// Calculates the delta for entity movement.
///
/// Returns `None` if the delta doesn't fit in i16 (requires full sync).
#[inline]
#[must_use]
pub fn calc_delta(current: f64, previous: f64) -> Option<i16> {
    let delta = encode_position(current) - encode_position(previous);
    if (MIN_DELTA..=MAX_DELTA).contains(&delta) {
        Some(delta as i16)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_matches_java_rounding() {
        // Java Math.round() rounds half towards positive infinity
        assert_eq!(encode_position(0.5 / 4096.0), 1); // 0.5 -> 1
        assert_eq!(encode_position(-0.5 / 4096.0), 0); // -0.5 -> 0 (not -1!)
        assert_eq!(encode_position(1.5 / 4096.0), 2);
        assert_eq!(encode_position(-1.5 / 4096.0), -1); // -1.5 -> -1 (not -2!)
    }

    #[test]
    fn test_calc_delta() {
        // Small movement should produce valid delta
        let delta = calc_delta(100.001, 100.0);
        assert!(delta.is_some());
        assert!(delta.unwrap().abs() < 100);

        // Movement larger than i16 max (32767/4096 ≈ 8 blocks) should fail
        let delta = calc_delta(10.0, 0.0); // 10 blocks = 40960 units > i16::MAX
        assert!(delta.is_none());
    }

    #[test]
    fn test_angle_byte() {
        assert_eq!(to_angle_byte(0.0), 0);
        assert_eq!(to_angle_byte(90.0), 64);
        // 180 * 256 / 360 = 128, which wraps to -128 as signed byte
        assert_eq!(to_angle_byte(180.0), -128);
        assert_eq!(to_angle_byte(-90.0), -64);
        assert_eq!(to_angle_byte(360.0), 0); // Full rotation wraps
    }
}
