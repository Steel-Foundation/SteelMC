//! This module contains the `GameProfile` struct, which is used to store information about a player's profile.
use serde::{Deserialize, Serialize};
use steel_protocol::packets::login::{GameProfileProperty, LoginGameProfile};
use uuid::Uuid;

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
