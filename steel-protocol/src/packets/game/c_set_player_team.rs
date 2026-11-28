//! Clientbound set player team packet.

use std::io::{Result, Write};

use serde::{Deserialize, Serialize};
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_PLAYER_TEAM;
use steel_utils::codec::VarInt;
use steel_utils::serial::{PrefixedWrite, WriteTo as WriteToTrait};
use text_components::TextComponent;

const METHOD_ADD: i8 = 0;
const METHOD_REMOVE: i8 = 1;
const METHOD_CHANGE: i8 = 2;
const METHOD_JOIN: i8 = 3;
const METHOD_LEAVE: i8 = 4;

const OPTION_FRIENDLY_FIRE: i8 = 0x01;
const OPTION_SEE_INVISIBLES: i8 = 0x02;

/// Team name tag (and death message) visibility, vanilla `Team.Visibility`.
#[derive(WriteTo, Serialize, Deserialize, Copy, Clone, Debug, PartialEq, Eq)]
#[write(as = VarInt)]
#[serde(rename_all = "camelCase")]
pub enum TeamVisibility {
    Always = 0,
    Never = 1,
    HideForOtherTeams = 2,
    HideForOwnTeam = 3,
}

/// Team entity collision rule, vanilla `Team.CollisionRule`.
#[derive(WriteTo, Serialize, Deserialize, Copy, Clone, Debug, PartialEq, Eq)]
#[write(as = VarInt)]
#[serde(rename_all = "camelCase")]
pub enum TeamCollisionRule {
    Always = 0,
    Never = 1,
    PushOtherTeams = 2,
    PushOwnTeam = 3,
}

/// Team color with vanilla network ids.
#[derive(WriteTo, Serialize, Deserialize, Copy, Clone, Debug, PartialEq, Eq)]
#[write(as = VarInt)]
#[serde(rename_all = "snake_case")]
pub enum TeamColor {
    Black = 0,
    DarkBlue = 1,
    DarkGreen = 2,
    DarkAqua = 3,
    DarkRed = 4,
    DarkPurple = 5,
    Gold = 6,
    Gray = 7,
    DarkGray = 8,
    Blue = 9,
    Green = 10,
    Aqua = 11,
    Red = 12,
    LightPurple = 13,
    Yellow = 14,
    White = 15,
}

/// Team option flags packed into one byte by vanilla `PlayerTeam.packOptions`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerTeamOptions {
    pub allow_friendly_fire: bool,
    pub see_friendly_invisibles: bool,
}

impl PlayerTeamOptions {
    const fn pack(self) -> i8 {
        let mut options = 0;
        if self.allow_friendly_fire {
            options |= OPTION_FRIENDLY_FIRE;
        }
        if self.see_friendly_invisibles {
            options |= OPTION_SEE_INVISIBLES;
        }
        options
    }
}

impl WriteToTrait for PlayerTeamOptions {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.pack().write(writer)
    }
}

/// Team presentation sent when a team is created or modified.
#[derive(WriteTo, Clone, Debug, PartialEq, Eq)]
pub struct PlayerTeamParameters {
    pub display_name: TextComponent,
    pub player_prefix: TextComponent,
    pub player_suffix: TextComponent,
    pub name_tag_visibility: TeamVisibility,
    pub collision_rule: TeamCollisionRule,
    pub color: Option<TeamColor>,
    pub options: PlayerTeamOptions,
}

/// Change applied to a client-side team.
///
/// Vanilla stores an integer method plus an optional parameter block and a
/// possibly empty player list; this enum only allows the fields each method
/// actually writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerTeamMethod {
    /// Creates the team with its parameters and initial members.
    Add {
        parameters: PlayerTeamParameters,
        players: Vec<String>,
    },
    /// Removes the team.
    Remove,
    /// Updates the team's parameters.
    Change(PlayerTeamParameters),
    /// Adds score holders to the team.
    Join(Vec<String>),
    /// Removes score holders from the team.
    Leave(Vec<String>),
}

impl PlayerTeamMethod {
    const fn id(&self) -> i8 {
        match self {
            Self::Add { .. } => METHOD_ADD,
            Self::Remove => METHOD_REMOVE,
            Self::Change(_) => METHOD_CHANGE,
            Self::Join(_) => METHOD_JOIN,
            Self::Leave(_) => METHOD_LEAVE,
        }
    }
}

/// Creates, modifies, removes, or changes the membership of a team.
#[derive(ClientPacket, Clone, Debug, PartialEq, Eq)]
#[packet_id(Play = C_SET_PLAYER_TEAM)]
pub struct CSetPlayerTeam {
    pub name: String,
    pub method: PlayerTeamMethod,
}

impl CSetPlayerTeam {
    /// Creates a packet for `name` applying `method`.
    #[must_use]
    pub fn new(name: impl Into<String>, method: PlayerTeamMethod) -> Self {
        Self {
            name: name.into(),
            method,
        }
    }
}

fn write_players(players: &[String], writer: &mut impl Write) -> Result<()> {
    VarInt::from(players.len() as i32).write(writer)?;
    for player in players {
        player.write_prefixed::<VarInt>(writer)?;
    }
    Ok(())
}

impl WriteToTrait for CSetPlayerTeam {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.name.write_prefixed::<VarInt>(writer)?;
        self.method.id().write(writer)?;
        match &self.method {
            PlayerTeamMethod::Add {
                parameters,
                players,
            } => {
                parameters.write(writer)?;
                write_players(players, writer)
            }
            PlayerTeamMethod::Remove => Ok(()),
            PlayerTeamMethod::Change(parameters) => parameters.write(writer),
            PlayerTeamMethod::Join(players) | PlayerTeamMethod::Leave(players) => {
                write_players(players, writer)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(packet: &CSetPlayerTeam) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet.write(&mut bytes).expect("packet should encode");
        bytes
    }

    #[test]
    fn remove_writes_only_name_and_method() {
        let packet = CSetPlayerTeam::new("red", PlayerTeamMethod::Remove);

        assert_eq!(encode(&packet), [3, b'r', b'e', b'd', 1]);
    }

    #[test]
    fn leave_writes_player_list_without_parameters() {
        let packet = CSetPlayerTeam::new("t", PlayerTeamMethod::Leave(vec!["ab".to_owned()]));

        assert_eq!(encode(&packet), [1, b't', 4, 1, 2, b'a', b'b']);
    }

    #[test]
    fn change_writes_parameters_without_player_list() {
        let parameters = PlayerTeamParameters {
            display_name: TextComponent::plain("t"),
            player_prefix: TextComponent::plain(""),
            player_suffix: TextComponent::plain(""),
            name_tag_visibility: TeamVisibility::HideForOwnTeam,
            collision_rule: TeamCollisionRule::PushOtherTeams,
            color: Some(TeamColor::Red),
            options: PlayerTeamOptions {
                allow_friendly_fire: true,
                see_friendly_invisibles: true,
            },
        };
        let mut expected = vec![1, b't', 2];
        parameters
            .write(&mut expected)
            .expect("parameters should encode");
        let packet = CSetPlayerTeam::new("t", PlayerTeamMethod::Change(parameters));

        let bytes = encode(&packet);

        assert_eq!(bytes, expected);
        assert_eq!(bytes[bytes.len() - 5..], [3, 2, 1, 12, 3]);
    }
}
