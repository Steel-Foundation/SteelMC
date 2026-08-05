//! Serverbound swing packet - sent when the player swings their arm.

use steel_macros::{ReadFrom, ServerPacket};
use steel_utils::types::InteractionHand;

/// Sent when the player swings their arm (attacks or interacts).
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SSwing {
    /// The hand used for the swing animation.
    pub hand: InteractionHand,
}
