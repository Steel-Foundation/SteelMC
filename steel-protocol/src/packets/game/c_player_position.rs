//! Clientbound player position packet - sent to teleport a player.
//!
//! The client must respond with `SAcceptTeleportation` containing the same teleport ID.

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

    /// All rotation flags
    pub const ALL_ROTATION: RelativeMovement = RelativeMovement(Self::Y_ROT | Self::X_ROT);

    /// Creates a new RelativeMovement with the given flags.
    #[must_use]
    pub const fn new(flags: i32) -> Self {
        Self(flags)
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
    /// Target X position
    pub x: f64,
    /// Target Y position
    pub y: f64,
    /// Target Z position
    pub z: f64,
    /// Target velocity X (delta movement)
    pub velocity_x: f64,
    /// Target velocity Y (delta movement)
    pub velocity_y: f64,
    /// Target velocity Z (delta movement)
    pub velocity_z: f64,
    /// Target yaw (Y rotation)
    pub yaw: f32,
    /// Target pitch (X rotation)
    pub pitch: f32,
    /// Relative movement flags
    pub relatives: RelativeMovement,
}

impl CPlayerPosition {
    /// Creates a new absolute teleport packet.
    #[must_use]
    pub fn absolute(teleport_id: i32, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) -> Self {
        Self {
            teleport_id,
            x,
            y,
            z,
            velocity_x: 0.0,
            velocity_y: 0.0,
            velocity_z: 0.0,
            yaw,
            pitch,
            relatives: RelativeMovement::NONE,
        }
    }

    /// Creates a teleport packet with relative rotation (keeps current rotation).
    #[must_use]
    pub fn with_relative_rotation(teleport_id: i32, x: f64, y: f64, z: f64) -> Self {
        Self {
            teleport_id,
            x,
            y,
            z,
            velocity_x: 0.0,
            velocity_y: 0.0,
            velocity_z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            relatives: RelativeMovement::ALL_ROTATION,
        }
    }
}
