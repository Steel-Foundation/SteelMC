//! Packet sent to spawn an entity (including players) for the client.

use steel_macros::ClientPacket;
use steel_registry::packets::play::C_ADD_ENTITY;
use steel_utils::codec::VarInt;
use steel_utils::serial::WriteTo;
use uuid::Uuid;

/// Spawns an entity on the client.
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_ADD_ENTITY)]
pub struct CAddEntity {
    /// The entity ID (used for all future references to this entity)
    pub id: i32,
    /// The entity's UUID
    pub uuid: Uuid,
    /// The entity type (from registry)
    pub entity_type: i32,
    /// X position
    pub x: f64,
    /// Y position
    pub y: f64,
    /// Z position
    pub z: f64,
    /// X velocity (blocks per tick)
    pub velocity_x: f64,
    /// Y velocity (blocks per tick)
    pub velocity_y: f64,
    /// Z velocity (blocks per tick)
    pub velocity_z: f64,
    /// Pitch (vertical rotation) as angle byte
    pub x_rot: i8,
    /// Yaw (horizontal rotation) as angle byte
    pub y_rot: i8,
    /// Head yaw as angle byte
    pub head_y_rot: i8,
    /// Entity data value (varies by entity type)
    pub data: i32,
}

// === LpVec3 encoding constants (from vanilla LpVec3.java) ===

/// Maximum absolute velocity value. Vanilla: `1.7179869183E10`
const ABS_MAX_VALUE: f64 = 1.717_986_918_3E10;

/// Minimum velocity threshold - below this, velocity is encoded as zero.
/// Vanilla: `3.051944088384301E-5`
const ABS_MIN_VALUE: f64 = 3.051944088384301E-5;

/// Maximum quantized value for component normalization. Vanilla: `32766.0`
const MAX_QUANTIZED_VALUE: f64 = 32766.0;

/// Continuation flag bit in LpVec3 encoding. Vanilla: `4`
const CONTINUATION_FLAG: i64 = 4;

impl WriteTo for CAddEntity {
    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        VarInt(self.id).write(writer)?;
        self.uuid.write(writer)?;
        VarInt(self.entity_type).write(writer)?;
        writer.write_all(&self.x.to_be_bytes())?;
        writer.write_all(&self.y.to_be_bytes())?;
        writer.write_all(&self.z.to_be_bytes())?;

        // Write velocity as LpVec3
        write_lp_vec3(writer, self.velocity_x, self.velocity_y, self.velocity_z)?;

        self.x_rot.write(writer)?;
        self.y_rot.write(writer)?;
        self.head_y_rot.write(writer)?;
        VarInt(self.data).write(writer)
    }
}

/// Writes a velocity vector in LpVec3 format.
///
/// Mirrors vanilla's `LpVec3.write()`.
///
/// Zero velocity is encoded as a single 0 byte.
/// Non-zero velocity uses 6+ bytes with bit-packed components.
pub fn write_lp_vec3(
    writer: &mut impl std::io::Write,
    x: f64,
    y: f64,
    z: f64,
) -> std::io::Result<()> {
    // Sanitize values (vanilla: LpVec3.sanitize)
    // NaN -> 0, clamp to [-ABS_MAX_VALUE, ABS_MAX_VALUE]
    let x = sanitize(x);
    let y = sanitize(y);
    let z = sanitize(z);

    // Chebyshev distance (max of absolute values)
    // Vanilla: Mth.absMax(x, Mth.absMax(y, z))
    let chessboard_length = x.abs().max(y.abs().max(z.abs()));

    if chessboard_length < ABS_MIN_VALUE {
        // Zero velocity - single byte
        writer.write_all(&[0u8])
    } else {
        // Non-zero velocity
        // Vanilla: long scale = Mth.ceilLong(chessboardLength);
        let scale = chessboard_length.ceil() as i64;

        // Check if scale needs more than 2 bits
        // Vanilla: boolean isPartial = (scale & 3L) != scale;
        let is_partial = (scale & 3) != scale;

        // Pack scale markers (2 low bits + optional continuation flag)
        // Vanilla: long markers = isPartial ? scale & 3L | 4L : scale;
        let markers: i64 = if is_partial {
            (scale & 3) | CONTINUATION_FLAG
        } else {
            scale
        };

        // Normalize and quantize components to 15-bit values [0, 32766]
        // Vanilla: long xn = pack(x / scale) << 3;
        let xn = pack_component(x / scale as f64);
        let yn = pack_component(y / scale as f64);
        let zn = pack_component(z / scale as f64);

        // Pack into 48 bits: markers(3) | x(15) | y(15) | z(15)
        // Vanilla: long buffer = markers | xn | yn | zn;
        let buffer: i64 = markers | (xn << 3) | (yn << 18) | (zn << 33);

        // Write 6 bytes
        // Vanilla:
        //   output.writeByte((byte)buffer);
        //   output.writeByte((byte)(buffer >> 8));
        //   output.writeInt((int)(buffer >> 16));
        writer.write_all(&[buffer as u8])?;
        writer.write_all(&[(buffer >> 8) as u8])?;
        writer.write_all(&((buffer >> 16) as u32).to_be_bytes())?;

        // Write high scale bits if needed
        // Vanilla: if (isPartial) { VarInt.write(output, (int)(scale >> 2)); }
        if is_partial {
            VarInt((scale >> 2) as i32).write(writer)?;
        }

        Ok(())
    }
}

/// Sanitizes a velocity component.
/// Mirrors vanilla's `LpVec3.sanitize()`.
#[inline]
fn sanitize(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-ABS_MAX_VALUE, ABS_MAX_VALUE)
    }
}

/// Pack a normalized [-1, 1] value to a 15-bit quantized value [0, 32766].
/// Mirrors vanilla's `LpVec3.pack()`: `Math.round((value * 0.5 + 0.5) * 32766.0)`
#[inline]
fn pack_component(value: f64) -> i64 {
    // Java's Math.round() rounds half towards positive infinity.
    // Use floor(x + 0.5) to match this behavior.
    let normalized = value * 0.5 + 0.5;
    (normalized * MAX_QUANTIZED_VALUE + 0.5).floor() as i64
}

impl CAddEntity {
    /// Creates a new CAddEntity packet for spawning a player.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn player(
        id: i32,
        uuid: Uuid,
        entity_type_id: i32,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
    ) -> Self {
        Self {
            id,
            uuid,
            entity_type: entity_type_id,
            x,
            y,
            z,
            velocity_x: 0.0,
            velocity_y: 0.0,
            velocity_z: 0.0,
            x_rot: super::to_angle_byte(pitch),
            y_rot: super::to_angle_byte(yaw),
            head_y_rot: super::to_angle_byte(yaw),
            data: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_velocity() {
        let mut buf = Vec::new();
        write_lp_vec3(&mut buf, 0.0, 0.0, 0.0).unwrap();
        assert_eq!(buf, vec![0]);
    }

    #[test]
    fn test_tiny_velocity_is_zero() {
        let mut buf = Vec::new();
        write_lp_vec3(&mut buf, 1e-6, 1e-6, 1e-6).unwrap();
        assert_eq!(buf, vec![0]);
    }

    #[test]
    fn test_non_zero_velocity() {
        let mut buf = Vec::new();
        write_lp_vec3(&mut buf, 1.0, 0.0, 0.0).unwrap();
        // Non-zero velocity should be 6 bytes (no continuation needed for scale=1)
        assert_eq!(buf.len(), 6);
    }

    #[test]
    fn test_pack_component_java_rounding() {
        // Java Math.round() rounds half towards positive infinity
        // pack(0.0) = round((0 * 0.5 + 0.5) * 32766) = round(16383) = 16383
        assert_eq!(pack_component(0.0), 16383);

        // pack(1.0) = round((1 * 0.5 + 0.5) * 32766) = round(32766) = 32766
        assert_eq!(pack_component(1.0), 32766);

        // pack(-1.0) = round((-1 * 0.5 + 0.5) * 32766) = round(0) = 0
        assert_eq!(pack_component(-1.0), 0);
    }

    #[test]
    fn test_sanitize() {
        assert_eq!(sanitize(0.0), 0.0);
        assert_eq!(sanitize(1.0), 1.0);
        assert_eq!(sanitize(-1.0), -1.0);
        assert_eq!(sanitize(f64::NAN), 0.0);
        assert_eq!(sanitize(f64::INFINITY), ABS_MAX_VALUE);
        assert_eq!(sanitize(f64::NEG_INFINITY), -ABS_MAX_VALUE);
    }

    #[test]
    fn test_velocity_with_scale() {
        // Test velocity that requires scale > 3 (continuation bit)
        let mut buf = Vec::new();
        write_lp_vec3(&mut buf, 5.0, 0.0, 0.0).unwrap();
        // scale=5, which is > 3, so needs continuation
        // First byte should have continuation flag set (bit 2)
        assert_eq!(buf[0] & 0x04, 0x04, "Continuation flag should be set");
        // Should be 6 bytes + VarInt for scale
        assert!(buf.len() > 6, "Should have continuation VarInt");
    }
}
