//! Serverbound packet for changing the held item slot.

use steel_macros::{ReadFrom, ServerPacket};

/// Sent by the client when the player changes their selected hotbar slot.
#[derive(ServerPacket, ReadFrom, Clone, Debug)]
pub struct SSetHeldItem {
    /// The slot index (0-8).
    pub slot: i16,
}
