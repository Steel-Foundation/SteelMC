use steel_macros::ClientPacket;
use steel_registry::packets::play::C_PLAYER_INFO_UPDATE;
use steel_utils::codec::VarInt;
use steel_utils::serial::PrefixedWrite;
use steel_utils::text::TextComponent;
use uuid::Uuid;

// Import RemoteChatSessionData for chat session transmission
use super::chat_session_data::ProtocolRemoteChatSessionData;
use crate::packets::login::GameProfileProperty;

/// Actions for the player info update packet.
/// These match the vanilla Java ClientboundPlayerInfoUpdatePacket.Action enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PlayerInfoAction {
    AddPlayer = 0x01,
    InitializeChat = 0x02,
    UpdateGameMode = 0x04,
    UpdateListed = 0x08,
    UpdateLatency = 0x10,
    UpdateDisplayName = 0x20,
    UpdateListOrder = 0x40,
    UpdateHat = 0x80,
}

/// Bitmask combining all actions needed when a player first joins.
/// This matches vanilla's createPlayerInitializing() method.
pub const PLAYER_INFO_INIT_ACTIONS: u8 = PlayerInfoAction::AddPlayer as u8
    | PlayerInfoAction::InitializeChat as u8
    | PlayerInfoAction::UpdateGameMode as u8
    | PlayerInfoAction::UpdateListed as u8
    | PlayerInfoAction::UpdateLatency as u8
    | PlayerInfoAction::UpdateDisplayName as u8
    | PlayerInfoAction::UpdateListOrder as u8
    | PlayerInfoAction::UpdateHat as u8;

/// Represents the display name state for a player.
#[derive(Debug, Clone)]
pub enum PlayerDisplayName {
    /// Use the player's default username (no custom display name).
    Reset,
    /// Use a custom display name.
    Custom(Box<TextComponent>),
}

impl From<Option<TextComponent>> for PlayerDisplayName {
    fn from(opt: Option<TextComponent>) -> Self {
        match opt {
            Some(component) => Self::Custom(Box::new(component)),
            None => Self::Reset,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlayerInfoEntry {
    pub uuid: Uuid,
    pub name: Option<String>,
    pub properties: Vec<GameProfileProperty>,
    pub chat_session: Option<ProtocolRemoteChatSessionData>,
    pub game_mode: Option<VarInt>,
    pub listed: Option<bool>,
    pub latency: Option<VarInt>,
    pub display_name: Option<PlayerDisplayName>,
    pub list_order: Option<VarInt>,
    pub show_hat: Option<bool>,
}

impl PlayerInfoEntry {
    /// Creates a new entry with only the UUID filled in.
    #[must_use]
    pub fn new(uuid: Uuid) -> Self {
        Self {
            uuid,
            name: None,
            properties: Vec::new(),
            chat_session: None,
            game_mode: None,
            listed: None,
            latency: None,
            display_name: None,
            list_order: None,
            show_hat: None,
        }
    }

    /// Sets the latency field.
    #[must_use]
    pub fn with_latency(mut self, latency: i32) -> Self {
        self.latency = Some(VarInt(latency));
        self
    }

    /// Sets the game mode field.
    #[must_use]
    pub fn with_game_mode(mut self, game_mode: i32) -> Self {
        self.game_mode = Some(VarInt(game_mode));
        self
    }

    /// Sets the listed field.
    #[must_use]
    pub fn with_listed(mut self, listed: bool) -> Self {
        self.listed = Some(listed);
        self
    }

    /// Sets the display name field.
    #[must_use]
    pub fn with_display_name(mut self, display_name: impl Into<PlayerDisplayName>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }

    /// Sets the show hat field.
    #[must_use]
    pub fn with_show_hat(mut self, show_hat: bool) -> Self {
        self.show_hat = Some(show_hat);
        self
    }

    /// Sets the list order field (controls sort order in tab list).
    #[must_use]
    pub fn with_list_order(mut self, list_order: i32) -> Self {
        self.list_order = Some(VarInt(list_order));
        self
    }

    /// Sets the chat session field.
    #[must_use]
    pub fn with_chat_session(mut self, chat_session: ProtocolRemoteChatSessionData) -> Self {
        self.chat_session = Some(chat_session);
        self
    }
}

#[derive(ClientPacket, Debug, Clone)]
#[packet_id(Play = C_PLAYER_INFO_UPDATE)]
pub struct CPlayerInfoUpdate {
    pub actions: u8, // Bitmask of PlayerInfoAction
    pub entries: Vec<PlayerInfoEntry>,
}

impl CPlayerInfoUpdate {
    /// Creates a full player initializing packet with all information.
    /// This is sent when a player joins to add them to the tab list.
    /// Matches vanilla's ClientboundPlayerInfoUpdatePacket.createPlayerInitializing()
    #[must_use]
    pub fn create_player_initializing(
        uuid: Uuid,
        name: String,
        properties: Vec<GameProfileProperty>,
        game_mode: i32,
        latency: i32,
        display_name: Option<TextComponent>,
        show_hat: bool,
    ) -> Self {
        Self {
            actions: PLAYER_INFO_INIT_ACTIONS,
            entries: vec![PlayerInfoEntry {
                uuid,
                name: Some(name),
                properties,
                chat_session: None,
                game_mode: Some(VarInt(game_mode)),
                listed: Some(true),
                latency: Some(VarInt(latency)),
                display_name: Some(display_name.into()),
                list_order: Some(VarInt(0)),
                show_hat: Some(show_hat),
            }],
        }
    }

    /// Creates a packet to update a player's chat session.
    #[must_use]
    pub fn update_chat_session(uuid: Uuid, chat_session: ProtocolRemoteChatSessionData) -> Self {
        Self {
            actions: PlayerInfoAction::InitializeChat as u8,
            entries: vec![PlayerInfoEntry::new(uuid).with_chat_session(chat_session)],
        }
    }

    /// Creates a packet to update latency for multiple players.
    /// This is sent periodically (every 600 ticks) to update ping display.
    #[must_use]
    pub fn update_latency(entries: Vec<(Uuid, i32)>) -> Self {
        Self {
            actions: PlayerInfoAction::UpdateLatency as u8,
            entries: entries
                .into_iter()
                .map(|(uuid, latency)| PlayerInfoEntry::new(uuid).with_latency(latency))
                .collect(),
        }
    }

    /// Creates a packet to update a player's game mode.
    #[must_use]
    pub fn update_game_mode(uuid: Uuid, game_mode: i32) -> Self {
        Self {
            actions: PlayerInfoAction::UpdateGameMode as u8,
            entries: vec![PlayerInfoEntry::new(uuid).with_game_mode(game_mode)],
        }
    }

    /// Creates a packet to update a player's listed status (show/hide in tab list).
    #[must_use]
    pub fn update_listed(uuid: Uuid, listed: bool) -> Self {
        Self {
            actions: PlayerInfoAction::UpdateListed as u8,
            entries: vec![PlayerInfoEntry::new(uuid).with_listed(listed)],
        }
    }

    /// Creates a packet to update a player's display name.
    #[must_use]
    pub fn update_display_name(uuid: Uuid, display_name: Option<TextComponent>) -> Self {
        Self {
            actions: PlayerInfoAction::UpdateDisplayName as u8,
            entries: vec![PlayerInfoEntry::new(uuid).with_display_name(display_name)],
        }
    }

    /// Creates a packet to update a player's hat visibility.
    #[must_use]
    pub fn update_hat(uuid: Uuid, show_hat: bool) -> Self {
        Self {
            actions: PlayerInfoAction::UpdateHat as u8,
            entries: vec![PlayerInfoEntry::new(uuid).with_show_hat(show_hat)],
        }
    }

    /// Creates a packet to update a player's list order (sort position in tab list).
    #[must_use]
    pub fn update_list_order(uuid: Uuid, list_order: i32) -> Self {
        Self {
            actions: PlayerInfoAction::UpdateListOrder as u8,
            entries: vec![PlayerInfoEntry::new(uuid).with_list_order(list_order)],
        }
    }
}

impl steel_utils::serial::WriteTo for CPlayerInfoUpdate {
    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        self.actions.write(writer)?;
        VarInt(self.entries.len() as i32).write(writer)?;

        for entry in &self.entries {
            entry.uuid.write(writer)?;

            if self.actions & (PlayerInfoAction::AddPlayer as u8) != 0
                && let Some(ref name) = entry.name
            {
                name.write_prefixed::<VarInt>(writer)?;
                // Write properties (including skin textures)
                VarInt(entry.properties.len() as i32).write(writer)?;
                for prop in &entry.properties {
                    prop.write(writer)?;
                }
            }

            if self.actions & (PlayerInfoAction::InitializeChat as u8) != 0 {
                // Write nullable chat session data
                if let Some(ref session_data) = entry.chat_session {
                    true.write(writer)?;
                    session_data.write(writer)?;
                } else {
                    false.write(writer)?;
                }
            }

            if self.actions & (PlayerInfoAction::UpdateGameMode as u8) != 0 {
                let game_mode = entry.game_mode.unwrap_or(VarInt(0));
                game_mode.write(writer)?;
            }

            if self.actions & (PlayerInfoAction::UpdateListed as u8) != 0 {
                let listed = entry.listed.unwrap_or(true);
                listed.write(writer)?;
            }

            if self.actions & (PlayerInfoAction::UpdateLatency as u8) != 0 {
                let latency = entry.latency.unwrap_or(VarInt(0));
                latency.write(writer)?;
            }

            if self.actions & (PlayerInfoAction::UpdateDisplayName as u8) != 0 {
                // Write as optional TextComponent (boolean + component if present)
                match &entry.display_name {
                    Some(PlayerDisplayName::Custom(display_name)) => {
                        true.write(writer)?;
                        display_name.write(writer)?;
                    }
                    Some(PlayerDisplayName::Reset) | None => {
                        false.write(writer)?;
                    }
                }
            }

            if self.actions & (PlayerInfoAction::UpdateListOrder as u8) != 0 {
                let list_order = entry.list_order.unwrap_or(VarInt(0));
                list_order.write(writer)?;
            }

            if self.actions & (PlayerInfoAction::UpdateHat as u8) != 0 {
                let show_hat = entry.show_hat.unwrap_or(true);
                show_hat.write(writer)?;
            }
        }

        Ok(())
    }
}
