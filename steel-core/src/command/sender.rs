//! Module defining the sender of a command.
use std::{fmt, sync::Arc};
use text_components::TextComponent;
use uuid::Uuid;

use crate::{
    player::{DomainResidenceToken, Player},
    server::Server,
};

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

/// Exact key used to coalesce suggestions from one live player residence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandSuggestionKey {
    Player {
        uuid: Uuid,
        session_address: usize,
        residence: Option<DomainResidenceToken>,
    },
    Console,
    Rcon,
}

/// Exact runtime owner of queued command work.
///
/// UUIDs remain the ordering key, while the player `Arc` and residence token
/// prevent an old session or an earlier domain stay from resuming work.
#[derive(Clone)]
pub(crate) struct CommandExecutionOwner {
    sender: CommandSender,
    player_residence: Option<DomainResidenceToken>,
}

impl CommandExecutionOwner {
    pub(crate) fn capture(sender: CommandSender, server: &Server) -> Self {
        let player_residence = sender.get_player().and_then(|player| {
            // Snapshot first so a concurrent detach cannot pair source
            // availability with the next domain residence.
            let residence = player.domain_residence_token();
            server.command_world_for_player(player).map(|_| residence)
        });
        Self {
            sender,
            player_residence,
        }
    }

    #[cfg(test)]
    pub(crate) fn non_player_for_test(sender: CommandSender) -> Self {
        assert!(
            sender.get_player().is_none(),
            "test helper only constructs non-player command owners"
        );
        Self {
            sender,
            player_residence: None,
        }
    }

    pub(crate) fn key(&self) -> CommandSenderKey {
        self.sender.key()
    }

    pub(crate) fn suggestion_key(&self) -> CommandSuggestionKey {
        match &self.sender {
            CommandSender::Player(player) => CommandSuggestionKey::Player {
                uuid: player.gameprofile.id,
                // The owner retains this Arc while queued, so its allocation
                // address cannot be reused by a replacement session.
                session_address: Arc::as_ptr(player) as usize,
                residence: self.player_residence,
            },
            CommandSender::Console => CommandSuggestionKey::Console,
            CommandSender::Rcon => CommandSuggestionKey::Rcon,
        }
    }

    pub(crate) const fn sender(&self) -> &CommandSender {
        &self.sender
    }

    pub(crate) fn is_current(&self, server: &Server) -> bool {
        let CommandSender::Player(player) = &self.sender else {
            return true;
        };
        let Some(residence) = self.player_residence else {
            return false;
        };
        server.command_world_for_player(player).is_some()
            && player.is_domain_residence_current(residence)
    }
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
