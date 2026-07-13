//! Module defining the sender of a command.
use std::{fmt, sync::Arc};
use text_components::TextComponent;
use uuid::Uuid;

use crate::player::Player;

/// The sender of a command.
#[derive(Clone)]
pub enum CommandSender {
    /// The command was sent by a player via the chat.
    Player(Arc<Player>),
    /// The command was sent via the server's console.
    Console,
    /// The command was sent via Rcon.
    Rcon,
}

/// Stable identity used to preserve top-level command ordering while work is suspended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CommandSenderKey {
    Player(Uuid),
    Console,
    Rcon,
}

impl CommandSender {
    pub(crate) fn key(&self) -> CommandSenderKey {
        match self {
            Self::Player(player) => CommandSenderKey::Player(player.gameprofile.id),
            Self::Console => CommandSenderKey::Console,
            Self::Rcon => CommandSenderKey::Rcon,
        }
    }

    /// Returns the player if the sender is a player.
    #[must_use]
    pub const fn get_player(&self) -> Option<&Arc<Player>> {
        match self {
            Self::Player(player) => Some(player),
            _ => None,
        }
    }

    /// Sends a system message to the command sender.
    pub fn send_message(&self, text: &TextComponent) {
        match self {
            Self::Player(player) => player.send_message(text),
            Self::Console => log::info!("{text}"),
            // TODO: Implement Rcon message sending
            Self::Rcon => log::warn!("Dropping Rcon command message until Rcon output is wired"),
        }
    }
}

impl fmt::Display for CommandSender {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Player(p) => &p.gameprofile.name,
                Self::Console => "Server",
                Self::Rcon => "Rcon",
            }
        )
    }
}
