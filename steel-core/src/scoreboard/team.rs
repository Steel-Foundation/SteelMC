//! Team presentation and rules stored by the scoreboard.

use serde::{Deserialize, Serialize};
use steel_protocol::packets::game::{
    PlayerTeamOptions, PlayerTeamParameters, TeamCollisionRule, TeamColor, TeamVisibility,
};
use text_components::TextComponent;

/// Mutable properties of a vanilla `PlayerTeam`, excluding its name and members.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamProperties {
    /// Name shown for the team itself.
    pub display_name: TextComponent,
    /// Text prepended to member names.
    pub player_prefix: TextComponent,
    /// Text appended to member names.
    pub player_suffix: TextComponent,
    /// Color applied to member names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<TeamColor>,
    /// Whether members can damage each other.
    pub allow_friendly_fire: bool,
    /// Whether members see invisible teammates as translucent.
    pub see_friendly_invisibles: bool,
    /// Who sees member name tags.
    pub name_tag_visibility: TeamVisibility,
    /// Who sees member death messages; server-side only.
    pub death_message_visibility: TeamVisibility,
    /// Which entities members push.
    pub collision_rule: TeamCollisionRule,
}

impl TeamProperties {
    /// Returns the properties of a newly created team named `name`.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            display_name: TextComponent::plain(name.to_owned()),
            player_prefix: TextComponent::plain(""),
            player_suffix: TextComponent::plain(""),
            color: None,
            allow_friendly_fire: true,
            see_friendly_invisibles: true,
            name_tag_visibility: TeamVisibility::Always,
            death_message_visibility: TeamVisibility::Always,
            collision_rule: TeamCollisionRule::Always,
        }
    }

    /// Returns the client-visible subset of these properties.
    #[must_use]
    pub fn packet_parameters(&self) -> PlayerTeamParameters {
        PlayerTeamParameters {
            display_name: self.display_name.clone(),
            player_prefix: self.player_prefix.clone(),
            player_suffix: self.player_suffix.clone(),
            name_tag_visibility: self.name_tag_visibility,
            collision_rule: self.collision_rule,
            color: self.color,
            options: PlayerTeamOptions {
                allow_friendly_fire: self.allow_friendly_fire,
                see_friendly_invisibles: self.see_friendly_invisibles,
            },
        }
    }
}
