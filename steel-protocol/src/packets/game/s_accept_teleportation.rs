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
}
