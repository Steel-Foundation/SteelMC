//! This module contains the `GameProfile` struct, which is used to store information about a player's profile.
use serde::{Deserialize, Serialize};
use steel_protocol::packets::login::{GameProfileProperty, LoginGameProfile};
use uuid::{Builder, Uuid, Variant, Version};

/// An enum representing a profile action.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GameProfileAction {
    /// The player has been forced to change their name.
    ForcedNameChange,
    /// The player is using a banned skin.
    UsingBannedSkin,
}

/// A struct representing a player's game profile.
#[derive(Deserialize, Clone, Debug)]
pub struct GameProfile {
    /// The player's UUID.
    pub id: Uuid,
    /// The player's name.
    pub name: String,
    /// A list of properties for the player's profile.
    pub properties: Vec<GameProfileProperty>,
    /// A list of profile actions for the player.
    #[serde(rename = "profileActions")]
    pub profile_actions: Option<Vec<GameProfileAction>>,
}

impl<'a> From<&'a GameProfile> for LoginGameProfile<'a> {
    fn from(profile: &'a GameProfile) -> Self {
        LoginGameProfile {
            id: profile.id,
            name: &profile.name,
            properties: &profile.properties,
        }
    }
}

/// Returns whether a name is valid for online login and profile lookup.
#[must_use]
pub fn is_valid_player_name(name: &str) -> bool {
    (3..=16).contains(&name.len())
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// Generates vanilla's deterministic offline-mode UUID for a player name.
#[must_use]
pub fn offline_uuid(username: &str) -> Uuid {
    Builder::from_md5_bytes(md5::compute(format!("OfflinePlayer:{username}")).0)
        .with_version(Version::Md5)
        .with_variant(Variant::RFC4122)
        .into_uuid()
}

#[cfg(test)]
mod tests {
    use super::{is_valid_player_name, offline_uuid};

    #[test]
    fn validates_vanilla_player_names() {
        assert!(is_valid_player_name("Steve"));
        assert!(is_valid_player_name("Alex_123"));
        assert!(!is_valid_player_name("ab"));
        assert!(!is_valid_player_name("name-with-dash"));
        assert!(!is_valid_player_name("way_too_long_player_name"));
    }

    #[test]
    fn offline_uuid_matches_vanilla_name_uuid() {
        assert_eq!(
            offline_uuid("Steve").to_string(),
            "5627dd98-e6be-3c21-b8a8-e92344183641"
        );
    }
}
