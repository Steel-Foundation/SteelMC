//! Client synchronization for domain scoreboard changes.
//!
//! Vanilla's `ServerScoreboard` broadcasts every change to all online players.
//! Steel scoreboards are domain-scoped, so changes only reach players in the
//! scoreboard's domain, and players are resynchronized when they switch domains.

use steel_protocol::packet_traits::EncodedPacket;
use steel_protocol::packets::game::{CSetPlayerTeam, PlayerTeamMethod};
use steel_protocol::utils::ConnectionProtocol;

use super::{
    ScoreHolder, Scoreboard, ScoreboardError, ScoreboardTeam, TeamMembershipChange, TeamProperties,
};
use crate::{player::Player, server::Server, server::worlds::WorldMap};

/// A domain scoreboard whose team changes are sent to that domain's players.
///
/// Every change holds the scoreboard's client sync lock until its packets are
/// sent, so concurrent callers cannot deliver packets out of state order.
pub struct ServerScoreboard<'a> {
    scoreboard: &'a Scoreboard,
    worlds: &'a WorldMap,
    domain: &'a str,
}

impl Server {
    /// Returns the client-synchronized scoreboard for a domain.
    #[must_use]
    pub fn scoreboard(&self, domain: &str) -> Option<ServerScoreboard<'_>> {
        let (domain, scoreboard) = self.scoreboards.get_key_value(domain)?;
        Some(ServerScoreboard::new(scoreboard, &self.worlds, domain))
    }

    /// Sends a domain's teams to a player entering it.
    pub(crate) fn send_domain_teams(&self, player: &Player, domain: &str) {
        if let Some(scoreboard) = self.scoreboard(domain) {
            scoreboard.send_teams_to(player);
        }
    }
}

impl<'a> ServerScoreboard<'a> {
    /// Wraps `scoreboard`, which must be the scoreboard owned by `domain`.
    pub(crate) const fn new(
        scoreboard: &'a Scoreboard,
        worlds: &'a WorldMap,
        domain: &'a str,
    ) -> Self {
        Self {
            scoreboard,
            worlds,
            domain,
        }
    }

    /// Returns the underlying scoreboard for reads.
    #[must_use]
    pub const fn scoreboard(&self) -> &'a Scoreboard {
        self.scoreboard
    }

    /// Adds a team with vanilla default properties.
    ///
    /// # Errors
    ///
    /// Returns an error if the team name is empty or already exists.
    pub fn add_team(&self, name: impl Into<String>) -> Result<ScoreboardTeam, ScoreboardError> {
        let _client_sync = self.scoreboard.client_sync.lock();
        let (team, properties) = self.scoreboard.add_team(name.into())?;
        self.broadcast(add_packet(team.name(), &properties, Vec::new()));
        Ok(team)
    }

    /// Removes a team; clients drop its members with it.
    ///
    /// # Errors
    ///
    /// Returns an error if the team no longer exists.
    pub fn remove_team(&self, team: &ScoreboardTeam) -> Result<(), ScoreboardError> {
        let _client_sync = self.scoreboard.client_sync.lock();
        self.scoreboard.remove_team(team)?;
        self.broadcast(CSetPlayerTeam::new(team.name(), PlayerTeamMethod::Remove));
        Ok(())
    }

    /// Applies `update` to a team's properties, sending them when they changed.
    ///
    /// # Errors
    ///
    /// Returns an error if the team no longer exists.
    pub fn update_team(
        &self,
        team: &ScoreboardTeam,
        update: impl FnOnce(&mut TeamProperties),
    ) -> Result<(), ScoreboardError> {
        let _client_sync = self.scoreboard.client_sync.lock();
        if let Some(properties) = self.scoreboard.update_team_properties(team, update)? {
            self.broadcast(CSetPlayerTeam::new(
                team.name(),
                PlayerTeamMethod::Change(properties.packet_parameters()),
            ));
        }
        Ok(())
    }

    /// Adds a holder to a team, leaving its previous team first.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty holder or a team that no longer exists.
    pub fn add_holder_to_team(
        &self,
        holder: &ScoreHolder,
        team: &ScoreboardTeam,
    ) -> Result<(), ScoreboardError> {
        let _client_sync = self.scoreboard.client_sync.lock();
        let TeamMembershipChange::Joined { previous } =
            self.scoreboard.add_holder_to_team(holder, team)?
        else {
            return Ok(());
        };
        if let Some(previous) = previous {
            self.broadcast(CSetPlayerTeam::new(
                previous,
                PlayerTeamMethod::Leave(vec![holder.name().to_owned()]),
            ));
        }
        self.broadcast(CSetPlayerTeam::new(
            team.name(),
            PlayerTeamMethod::Join(vec![holder.name().to_owned()]),
        ));
        Ok(())
    }

    /// Removes a holder from its team and returns whether it was on one.
    #[must_use]
    pub fn remove_holder_from_team(&self, holder: &ScoreHolder) -> bool {
        let _client_sync = self.scoreboard.client_sync.lock();
        let Some(team) = self.scoreboard.remove_holder_from_team(holder) else {
            return false;
        };
        self.broadcast(CSetPlayerTeam::new(
            team,
            PlayerTeamMethod::Leave(vec![holder.name().to_owned()]),
        ));
        true
    }

    /// Sends every team and its members to a player entering this domain.
    pub fn send_teams_to(&self, player: &Player) {
        let _client_sync = self.scoreboard.client_sync.lock();
        for (name, properties, players) in self.scoreboard.teams_with_members() {
            player.send_packet(add_packet(&name, &properties, players));
        }
    }

    /// Removes every team from a player leaving this domain.
    pub fn send_team_removals_to(&self, player: &Player) {
        let _client_sync = self.scoreboard.client_sync.lock();
        for name in self.scoreboard.team_names() {
            player.send_packet(CSetPlayerTeam::new(name, PlayerTeamMethod::Remove));
        }
    }

    fn broadcast(&self, packet: CSetPlayerTeam) {
        let worlds = self.worlds.worlds_in_domain(self.domain);
        let Some(first) = worlds.first() else {
            return;
        };
        // Compression comes from server config, so one encoding fits every world.
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, first.compression, ConnectionProtocol::Play)
        else {
            return;
        };
        for world in &worlds {
            world.broadcast_to_all_encoded(encoded.clone());
        }
    }
}

fn add_packet(name: &str, properties: &TeamProperties, players: Vec<String>) -> CSetPlayerTeam {
    CSetPlayerTeam::new(
        name,
        PlayerTeamMethod::Add {
            parameters: properties.packet_parameters(),
            players,
        },
    )
}
