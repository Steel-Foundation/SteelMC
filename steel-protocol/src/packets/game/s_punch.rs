//! Serverbound punch packet - sent when the player swings their main hand.

use steel_macros::{ReadFrom, ServerPacket};

/// Sent when the player punches. The packet carries no payload; the swing is
/// always the main hand.
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SPunch {}
