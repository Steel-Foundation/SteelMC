use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_CHANGE_DIFFICULTY;
use steel_utils::types::Difficulty;

/// Clientbound packet that informs the client about the current world difficulty.
///
/// Sent during login, respawn, and whenever the difficulty changes.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_CHANGE_DIFFICULTY)]
pub struct CChangeDifficulty {
    /// The difficulty level.
    pub difficulty: Difficulty,
    /// Whether the difficulty is locked (prevents players from changing it).
    pub locked: bool,
}
