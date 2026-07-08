//! Clientbound player position packet - sent to teleport a player.
//!
//! The client must respond with `SAcceptTeleportation` containing the same teleport ID.

use glam::DVec3;
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_PLAYER_POSITION;

/// Relative position/rotation flags.
///
/// When a flag is set, the corresponding value is relative to the player's current value.
/// When not set, the value is absolute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RelativeMovement(pub i32);

impl RelativeMovement {
    /// X position is relative
    pub const X: i32 = 1 << 0;
    /// Y position is relative
    pub const Y: i32 = 1 << 1;
    /// Z position is relative
    pub const Z: i32 = 1 << 2;
    /// Y rotation (yaw) is relative
    pub const Y_ROT: i32 = 1 << 3;
    /// X rotation (pitch) is relative
    pub const X_ROT: i32 = 1 << 4;
    /// Delta X is relative
    pub const DELTA_X: i32 = 1 << 5;
    /// Delta Y is relative
    pub const DELTA_Y: i32 = 1 << 6;
    /// Delta Z is relative
    pub const DELTA_Z: i32 = 1 << 7;
    /// Rotate delta is relative
    pub const ROTATE_DELTA: i32 = 1 << 8;

    /// No relative flags (all values are absolute)
    pub const NONE: RelativeMovement = RelativeMovement(0);

    /// All rotation flags.
    pub const ROTATION: RelativeMovement = RelativeMovement(Self::Y_ROT | Self::X_ROT);

    /// Vanilla delta movement flags, including rotated-delta semantics.
    pub const DELTA: RelativeMovement =
        RelativeMovement(Self::DELTA_X | Self::DELTA_Y | Self::DELTA_Z | Self::ROTATE_DELTA);

    /// Creates a new RelativeMovement with the given flags.
    #[must_use]
    pub const fn new(flags: i32) -> Self {
        Self(flags)
    }

    /// Returns the union of two relative movement sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns true if the X position is relative.
    #[must_use]
    pub const fn is_x_relative(self) -> bool {
        self.0 & Self::X != 0
    }

    /// Returns true if the Y position is relative.
    #[must_use]
    pub const fn is_y_relative(self) -> bool {
        self.0 & Self::Y != 0
    }

    /// Returns true if the Z position is relative.
    #[must_use]
    pub const fn is_z_relative(self) -> bool {
        self.0 & Self::Z != 0
    }

    /// Returns true if yaw is relative.
    #[must_use]
    pub const fn is_y_rot_relative(self) -> bool {
        self.0 & Self::Y_ROT != 0
    }

    /// Returns true if pitch is relative.
    #[must_use]
    pub const fn is_x_rot_relative(self) -> bool {
        self.0 & Self::X_ROT != 0
    }

    /// Returns true if delta X is relative.
    #[must_use]
    pub const fn is_delta_x_relative(self) -> bool {
        self.0 & Self::DELTA_X != 0
    }

    /// Returns true if delta Y is relative.
    #[must_use]
    pub const fn is_delta_y_relative(self) -> bool {
        self.0 & Self::DELTA_Y != 0
    }

    /// Returns true if delta Z is relative.
    #[must_use]
    pub const fn is_delta_z_relative(self) -> bool {
        self.0 & Self::DELTA_Z != 0
    }

    /// Returns true if current delta movement is rotated by the teleport rotation delta.
    #[must_use]
    pub const fn rotates_delta(self) -> bool {
        self.0 & Self::ROTATE_DELTA != 0
    }
}

impl steel_utils::serial::WriteTo for RelativeMovement {
    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        self.0.write(writer)
    }
}

/// Sent to teleport a player to a new position.
///
/// The client must acknowledge this packet by sending `SAcceptTeleportation`
/// with the same teleport ID. Until acknowledged, the server will reject
/// position updates from the client.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_PLAYER_POSITION)]
pub struct CPlayerPosition {
    /// Unique teleport ID that must be echoed back by the client.
    #[write(as = VarInt)]
    pub teleport_id: i32,
    /// Target position
    pub pos: DVec3,
    /// Target velocity (delta movement)
    pub vel: DVec3,
    /// Target yaw (Y rotation)
    pub yaw: f32,
    /// Target pitch (X rotation)
    pub pitch: f32,
    /// Relative movement flags
    pub relatives: RelativeMovement,
}

impl CPlayerPosition {
    /// Creates a teleport packet with explicit relative flags.
    #[must_use]
    pub const fn new(
        teleport_id: i32,
        pos: DVec3,
        vel: DVec3,
        yaw: f32,
        pitch: f32,
        relatives: RelativeMovement,
    ) -> Self {
        Self {
            teleport_id,
            pos,
            vel,
            yaw,
            pitch,
            relatives,
        }
    }

    /// Creates a new absolute teleport packet.
    #[must_use]
    pub fn absolute(teleport_id: i32, pos: DVec3, yaw: f32, pitch: f32) -> Self {
        Self::absolute_with_velocity(teleport_id, pos, DVec3::ZERO, yaw, pitch)
    }

    /// Creates a new absolute teleport packet with explicit delta movement.
    #[must_use]
    pub const fn absolute_with_velocity(
        teleport_id: i32,
        pos: DVec3,
        vel: DVec3,
        yaw: f32,
        pitch: f32,
    ) -> Self {
        Self::new(teleport_id, pos, vel, yaw, pitch, RelativeMovement::NONE)
    }

    /// Creates a teleport packet with relative rotation (keeps current rotation).
    #[must_use]
    pub fn with_relative_rotation(teleport_id: i32, pos: DVec3) -> Self {
        Self {
            teleport_id,
            pos,
            vel: DVec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            relatives: RelativeMovement::ROTATION,
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::{CPlayerPosition, RelativeMovement};

    #[test]
    fn delta_matches_vanilla_relative_delta_set() {
        assert_eq!(
            RelativeMovement::DELTA.0,
            RelativeMovement::DELTA_X
                | RelativeMovement::DELTA_Y
                | RelativeMovement::DELTA_Z
                | RelativeMovement::ROTATE_DELTA
        );
    }

    #[test]
    fn absolute_with_velocity_preserves_delta_movement() {
        let packet = CPlayerPosition::absolute_with_velocity(
            12,
            DVec3::new(1.0, 2.0, 3.0),
            DVec3::new(0.1, 0.2, 0.3),
            45.0,
            -10.0,
        );

        assert_eq!(packet.teleport_id, 12);
        assert_eq!(packet.pos, DVec3::new(1.0, 2.0, 3.0));
        assert_eq!(packet.vel, DVec3::new(0.1, 0.2, 0.3));
        assert_eq!(packet.yaw, 45.0);
        assert_eq!(packet.pitch, -10.0);
        assert_eq!(packet.relatives, RelativeMovement::NONE);
    }
}
