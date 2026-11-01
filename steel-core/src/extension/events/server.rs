//! Server-level events.

use std::sync::Arc;

use steel_utils::Identifier;

use super::Event;
use crate::player::Player;

/// Who sent a non-player command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandSenderKind {
    /// The server console.
    Console,
    /// A remote console connection.
    Rcon,
}

/// A console or remote command, before parsing.
pub struct ServerCommandEvent {
    /// Sender.
    pub sender: CommandSenderKind,
    /// Command line without a leading `/`.
    pub command: String,
    /// Whether the command is dropped.
    pub cancelled: bool,
}

impl Event for ServerCommandEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A client requests the server list entry. Runs on the connection task.
pub struct ServerListPingEvent {
    /// Message of the day, plain text with `§` formatting codes as in `server.motd`.
    pub motd: String,
    /// Displayed player limit.
    pub max_players: i32,
    /// Displayed online count.
    pub online_players: i32,
}

impl Event for ServerListPingEvent {}

/// A player sent a extension-channel (custom payload) message.
pub struct PluginMessageEvent {
    /// Sending player.
    pub player: Arc<Player>,
    /// Channel identifier.
    pub channel: Identifier,
    /// Raw payload.
    pub data: Vec<u8>,
}

impl Event for PluginMessageEvent {}
