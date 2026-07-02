//! Packet sent to spawn an entity (including players) for the client.

use glam::DVec3;
use steel_macros::ClientPacket;
use steel_registry::packets::play::C_ADD_ENTITY;
use steel_utils::codec::{LpVec3, VarInt};
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
    /// The entity position
    pub position: DVec3,
    /// The entity velocity (blocks per tick)
    pub velocity: DVec3,
    /// Pitch (vertical rotation) as angle byte
    pub x_rot: i8,
    /// Yaw (horizontal rotation) as angle byte
    pub y_rot: i8,
    /// Head yaw as angle byte
    pub head_y_rot: i8,
    /// Entity data value (varies by entity type)
    pub data: i32,
}

impl WriteTo for CAddEntity {
    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        VarInt(self.id).write(writer)?;
        self.uuid.write(writer)?;
        VarInt(self.entity_type).write(writer)?;
        writer.write_all(&self.position.x.to_be_bytes())?;
        writer.write_all(&self.position.y.to_be_bytes())?;
        writer.write_all(&self.position.z.to_be_bytes())?;

        // Write velocity as LpVec3
        write_lp_vec3(writer, self.velocity)?;

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
pub fn write_lp_vec3(writer: &mut impl std::io::Write, velocity: DVec3) -> std::io::Result<()> {
    LpVec3(velocity).write(writer)
}

impl CAddEntity {
    /// Creates a new CAddEntity packet for spawning a player.
    #[must_use]
    pub const fn player(
        id: i32,
        uuid: Uuid,
        entity_type_id: i32,
        position: DVec3,
        yaw: f32,
        pitch: f32,
    ) -> Self {
        Self {
            id,
            uuid,
            entity_type: entity_type_id,
            position,
            velocity: DVec3::ZERO,
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
        write_lp_vec3(&mut buf, DVec3::ZERO).unwrap();
        assert_eq!(buf, vec![0]);
    }

    #[test]
    fn test_tiny_velocity_is_zero() {
        let mut buf = Vec::new();
        write_lp_vec3(&mut buf, DVec3::splat(1e-6)).unwrap();
        assert_eq!(buf, vec![0]);
    }

    #[test]
    fn test_non_zero_velocity() {
        let mut buf = Vec::new();
        write_lp_vec3(&mut buf, DVec3::ZERO.with_x(1.0)).unwrap();
        // Non-zero velocity should be 6 bytes (no continuation needed for scale=1)
        assert_eq!(buf.len(), 6);
    }

    #[test]
    fn test_negative_velocity_uses_absolute_scale() {
        let mut buf = Vec::new();
        write_lp_vec3(&mut buf, DVec3::ZERO.with_x(-1.0)).unwrap();
        assert_eq!(buf.len(), 6);
    }

    #[test]
    fn test_velocity_with_scale() {
        // Test velocity that requires scale > 3 (continuation bit)
        let mut buf = Vec::new();
        write_lp_vec3(&mut buf, DVec3::ZERO.with_x(5.0)).unwrap();
        // scale=5, which is > 3, so needs continuation
        // First byte should have continuation flag set (bit 2)
        assert_eq!(buf[0] & 0x04, 0x04, "Continuation flag should be set");
        // Should be 6 bytes + VarInt for scale
        assert!(buf.len() > 6, "Should have continuation VarInt");
    }
}
