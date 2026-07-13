use glam::DVec3;
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_TELEPORT_ENTITY;

use super::c_player_position::RelativeMovement;

/// Teleports an entity with optional relative position, rotation, and velocity flags.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_TELEPORT_ENTITY)]
pub struct CTeleportEntity {
    #[write(as = VarInt)]
    pub entity_id: i32,
    pub pos: DVec3,
    pub vel: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub relatives: RelativeMovement,
    pub on_ground: bool,
}

impl CTeleportEntity {
    #[must_use]
    pub const fn new(
        entity_id: i32,
        pos: DVec3,
        vel: DVec3,
        yaw: f32,
        pitch: f32,
        relatives: RelativeMovement,
        on_ground: bool,
    ) -> Self {
        Self {
            entity_id,
            pos,
            vel,
            yaw,
            pitch,
            relatives,
            on_ground,
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::{CTeleportEntity, RelativeMovement};

    #[test]
    fn teleport_entity_preserves_relative_flags_and_motion() {
        let packet = CTeleportEntity::new(
            42,
            DVec3::new(1.0, 2.0, 3.0),
            DVec3::new(0.1, 0.2, 0.3),
            90.0,
            -15.0,
            RelativeMovement::DELTA.union(RelativeMovement::ROTATION),
            true,
        );

        assert_eq!(packet.entity_id, 42);
        assert_eq!(packet.pos, DVec3::new(1.0, 2.0, 3.0));
        assert_eq!(packet.vel, DVec3::new(0.1, 0.2, 0.3));
        assert_eq!(packet.yaw, 90.0);
        assert_eq!(packet.pitch, -15.0);
        assert_eq!(
            packet.relatives,
            RelativeMovement::DELTA.union(RelativeMovement::ROTATION)
        );
        assert!(packet.on_ground);
    }
}
