//! Packets for entity movement updates.
//!
//! These packets use fixed-point encoding for position deltas. The client maintains
//! a `VecDeltaCodec` for each entity which tracks the "base" position. Deltas are
//! computed as `encode(current) - encode(base)` where encode multiplies by 4096
//! and rounds.
//!
//! The server must track what the client's base position is (`PositionCodec`) to
//! compute correct deltas and know when the delta would overflow i16 bounds.

use std::io::{self, Write};

use steel_macros::{ClientPacket, WriteTo};
use steel_math::DEGREE_360;
use steel_registry::packets::play::{C_MOVE_ENTITY_POS, C_MOVE_ENTITY_POS_ROT, C_MOVE_ENTITY_ROT};

/// Fixed-point encoding multiplier (1/4096 block precision).
const TRUNCATION_STEPS: f64 = 4096.0;

/// Maximum delta value that fits in i16.
const MAX_DELTA: i64 = i16::MAX as i64;

/// Minimum delta value that fits in i16.
const MIN_DELTA: i64 = i16::MIN as i64;

/// Updates an entity's position with a delta from its current position.
///
/// Vanilla packs this as `entityId, properties, VecDelta`, where `properties`
/// is a `VarInt` combining the on-ground flag (bit 0) with a step count (upper
/// bits) used for the client's sub-tick movement-interpolation paths. Steel
/// never emits interpolation steps, so the step count is always 0 and
/// `properties` reduces to just the on-ground flag as a `VarInt`.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_MOVE_ENTITY_POS)]
pub struct CMoveEntityPos {
    #[write(as = VarInt)]
    pub entity_id: i32,
    #[write(as = VarInt)]
    pub on_ground: bool,
    /// Delta X (current X * 4096 - previous X * 4096)
    pub dx: PackedEntityDelta,
    /// Delta Y
    pub dy: PackedEntityDelta,
    /// Delta Z
    pub dz: PackedEntityDelta,
}

/// Updates an entity's position and rotation.
///
/// See [`CMoveEntityPos`] for why `on_ground` is written as a `VarInt`
/// "properties" field ahead of the position delta rather than as a trailing
/// boolean.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_MOVE_ENTITY_POS_ROT)]
pub struct CMoveEntityPosRot {
    #[write(as = VarInt)]
    pub entity_id: i32,
    #[write(as = VarInt)]
    pub on_ground: bool,
    /// Delta X (current X * 4096 - previous X * 4096)
    pub dx: PackedEntityDelta,
    /// Delta Y
    pub dy: PackedEntityDelta,
    /// Delta Z
    pub dz: PackedEntityDelta,
    /// Yaw as angle byte
    pub y_rot: i8,
    /// Pitch as angle byte
    pub x_rot: i8,
}

/// A fixed-point entity movement delta encoded as a protocol `i16`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedEntityDelta(i16);

impl PackedEntityDelta {
    /// Creates a packed entity delta from its raw protocol representation.
    #[must_use]
    pub const fn from_raw(raw: i16) -> Self {
        Self(raw)
    }

    /// Returns the raw protocol representation.
    #[must_use]
    pub const fn as_i16(self) -> i16 {
        self.0
    }

    /// Calculates a packed movement delta between two absolute coordinates.
    ///
    /// Returns `None` if the delta doesn't fit in the protocol's `i16` range.
    #[must_use]
    pub fn between(current: f64, previous: f64) -> Option<Self> {
        let delta = encode_position(current) - encode_position(previous);
        if (MIN_DELTA..=MAX_DELTA).contains(&delta) {
            Some(Self(delta as i16))
        } else {
            None
        }
    }
}

impl steel_utils::serial::WriteTo for PackedEntityDelta {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        steel_utils::serial::WriteTo::write(&self.0, writer)
    }
}

/// Updates an entity's rotation only.
///
/// Unlike [`CMoveEntityPos`]/[`CMoveEntityPosRot`], vanilla writes `on_ground`
/// here as a plain boolean (not a packed `VarInt`), and before the angle
/// bytes rather than after.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_MOVE_ENTITY_ROT)]
pub struct CMoveEntityRot {
    #[write(as = VarInt)]
    pub entity_id: i32,
    pub on_ground: bool,
    /// Yaw as angle byte
    pub y_rot: i8,
    /// Pitch as angle byte
    pub x_rot: i8,
}

/// Converts degrees to a protocol angle byte (0-255 representing 0-360 degrees).
///
/// Mirrors vanilla's `Mth.packDegrees()`: `(byte)floor(angle * 256.0F / 360.0F)`
#[inline]
#[must_use]
pub const fn to_angle_byte(degrees: f32) -> i8 {
    // Vanilla: (byte)floor(angle * 256.0F / 360.0F)
    // Cast to i32 first (safe for all angle values), then truncate to i8.
    // This matches Java's (byte) cast which truncates the low 8 bits.
    (degrees * 256.0 / DEGREE_360).floor() as i32 as i8
}

/// Encodes a position component to the protocol's fixed-point format.
///
/// Mirrors vanilla's `VecDeltaCodec.encode()` which uses `Math.round()`.
/// Java's `Math.round()` rounds half towards positive infinity (half-up),
/// which differs from Rust's `round()` that rounds half away from zero.
#[inline]
#[must_use]
pub const fn encode_position(value: f64) -> i64 {
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
pub fn calc_delta(current: f64, previous: f64) -> Option<PackedEntityDelta> {
    PackedEntityDelta::between(current, previous)
}

#[cfg(test)]
mod tests {
    use steel_utils::serial::WriteTo as _;

    use super::*;

    // Vanilla wire format (`ClientboundMoveEntityPacket.Pos/PosRot.write`, `VecDelta.write`
    // for a zero-step-count `Linear` delta): entityId VarInt, `properties` VarInt (on-ground
    // flag packed into bit 0, step count always 0 here), then dx/dy/dz as raw big-endian
    // shorts — on_ground is NOT a trailing boolean like the old (pre-step-interpolation)
    // format this packet used to have.
    #[test]
    fn pos_packet_matches_vanilla_wire_format() {
        let packet = CMoveEntityPos {
            entity_id: 5,
            on_ground: true,
            dx: PackedEntityDelta::from_raw(1),
            dy: PackedEntityDelta::from_raw(-1),
            dz: PackedEntityDelta::from_raw(300),
        };
        let mut buf = Vec::new();
        packet.write(&mut buf).unwrap();

        assert_eq!(
            buf,
            vec![
                5, // entity_id VarInt
                1, // properties VarInt: on_ground=1, step_count=0
                0, 1, // dx = 1
                0xFF, 0xFF, // dy = -1
                1, 44, // dz = 300
            ]
        );
    }

    #[test]
    fn pos_rot_packet_matches_vanilla_wire_format() {
        let packet = CMoveEntityPosRot {
            entity_id: 5,
            on_ground: false,
            dx: PackedEntityDelta::from_raw(0),
            dy: PackedEntityDelta::from_raw(0),
            dz: PackedEntityDelta::from_raw(0),
            y_rot: 64,
            x_rot: -64,
        };
        let mut buf = Vec::new();
        packet.write(&mut buf).unwrap();

        assert_eq!(
            buf,
            vec![
                5, // entity_id VarInt
                0, // properties VarInt: on_ground=0, step_count=0
                0, 0, 0, 0, 0, 0, // dx, dy, dz = 0
                64, // y_rot
                192, // x_rot (-64 as u8)
            ]
        );
    }

    // Vanilla `ClientboundMoveEntityPacket.Rot.write`: entityId VarInt, on_ground as a
    // plain boolean (unlike Pos/PosRot, not packed into a VarInt), then the angle bytes —
    // on_ground comes before the rotation, not after it.
    #[test]
    fn rot_packet_matches_vanilla_wire_format() {
        let packet = CMoveEntityRot {
            entity_id: 5,
            on_ground: true,
            y_rot: 64,
            x_rot: -64,
        };
        let mut buf = Vec::new();
        packet.write(&mut buf).unwrap();

        assert_eq!(buf, vec![5, 1, 64, 192]);
    }

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
        assert!(delta.unwrap().as_i16().abs() < 100);

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
