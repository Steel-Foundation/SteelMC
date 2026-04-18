use steel_macros::{ReadFrom, ServerPacket};
use steel_utils::types::Difficulty;

/// Serverbound packet sent when the client requests a difficulty change.
///
/// This is sent when the player changes the difficulty in the settings screen.
/// The server should validate permissions before applying the change.
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SChangeDifficulty {
    /// The requested difficulty level.
    pub difficulty: Difficulty,
}
