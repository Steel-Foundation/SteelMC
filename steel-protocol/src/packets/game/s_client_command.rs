//! Serverbound client command packet - sent by the client to perform actions like respawning.

use steel_macros::{ReadFrom, ServerPacket};

/// The action the client wants to perform.
#[derive(ReadFrom, Clone, Copy, Debug, PartialEq, Eq)]
#[read(as = VarInt)]
pub enum ClientCommandAction {
    /// The client clicked "Respawn" on the death screen.
    PerformRespawn = 0,
    /// The client is requesting stats (F3+F4 or similar).
    RequestStats = 1,
    /// The client is requesting game rule values.
    RequestGameRuleValues = 2,
}

/// Sent by the client when it wants to perform an action such as respawning after death.
///
/// When the player dies and sees the death screen, clicking "Respawn" sends this
/// packet with `action = PerformRespawn`.
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SClientCommand {
    /// The action the client wants to perform.
    pub action: ClientCommandAction,
}
