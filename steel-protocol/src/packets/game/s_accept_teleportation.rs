//! Serverbound accept teleportation packet - sent by client to acknowledge a teleport.

use steel_macros::{ReadFrom, ServerPacket};

/// Sent by the client to acknowledge a server-initiated teleport.
///
/// The client sends this after receiving a `CPlayerPosition` packet.
/// The teleport ID must match the one from the `CPlayerPosition` packet.
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SAcceptTeleportation {
    /// The teleport ID from the `CPlayerPosition` packet being acknowledged.
    #[read(as = VarInt)]
    pub teleport_id: i32,
    /// The X position the client ended up at.
    pub x: f64,
    /// The Y position the client ended up at.
    pub y: f64,
    /// The Z position the client ended up at.
    pub z: f64,
    /// The yaw the client ended up at.
    pub y_rot: f32,
    /// The pitch the client ended up at.
    pub x_rot: f32,
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use steel_utils::serial::ReadFrom as _;

    use super::*;

    #[test]
    fn accept_teleportation_reads_position_after_id() {
        let mut bytes = vec![1]; // teleport id as a VarInt
        for value in [1.5f64, 64.0, -2.5] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&90.0f32.to_be_bytes());
        bytes.extend_from_slice(&(-45.0f32).to_be_bytes());

        let packet =
            SAcceptTeleportation::read(&mut Cursor::new(&bytes[..])).expect("should decode");

        assert_eq!(packet.teleport_id, 1);
        assert_eq!((packet.x, packet.y, packet.z), (1.5, 64.0, -2.5));
        assert_eq!((packet.y_rot, packet.x_rot), (90.0, -45.0));
    }
}
