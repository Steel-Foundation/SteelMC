//! Login and configuration packet sequencing.

use std::{
    fmt::{self, Display, Formatter},
    mem,
};

use steel_core::player::GameProfile;
use steel_protocol::utils::ConnectionProtocol;

/// Exact state within the broad login protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoginPhase {
    Hello,
    Key,
    Authenticating,
    ProtocolSwitching,
}

/// Active vanilla configuration task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfigurationPhase {
    SynchronizeRegistries,
    JoinWorld,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrePlayPhase {
    Handshake,
    Status,
    Login(LoginPhase),
    Configuration(ConfigurationPhase),
    Play,
}

impl Display for PrePlayPhase {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Handshake => formatter.write_str("handshake"),
            Self::Status => formatter.write_str("status"),
            Self::Login(LoginPhase::Hello) => formatter.write_str("login hello"),
            Self::Login(LoginPhase::Key) => formatter.write_str("login key"),
            Self::Login(LoginPhase::Authenticating) => formatter.write_str("login authentication"),
            Self::Login(LoginPhase::ProtocolSwitching) => {
                formatter.write_str("login protocol switching")
            }
            Self::Configuration(ConfigurationPhase::SynchronizeRegistries) => {
                formatter.write_str("known-pack negotiation")
            }
            Self::Configuration(ConfigurationPhase::JoinWorld) => {
                formatter.write_str("configuration finish")
            }
            Self::Play => formatter.write_str("play"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrePlayPacket {
    ClientIntention,
    Hello,
    Key,
    LoginAcknowledged,
    SelectKnownPacks,
    FinishConfiguration,
}

impl Display for PrePlayPacket {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClientIntention => formatter.write_str("client intention"),
            Self::Hello => formatter.write_str("hello packet"),
            Self::Key => formatter.write_str("key packet"),
            Self::LoginAcknowledged => formatter.write_str("login acknowledgement"),
            Self::SelectKnownPacks => formatter.write_str("known-pack selection"),
            Self::FinishConfiguration => formatter.write_str("configuration acknowledgement"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PacketSequenceError {
    packet: PrePlayPacket,
    phase: PrePlayPhase,
}

impl Display for PacketSequenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "received {} during {}", self.packet, self.phase)
    }
}

#[derive(Debug)]
enum LoginState {
    Hello,
    Key { requested_username: String },
    Authenticating,
    ProtocolSwitching { authenticated_profile: GameProfile },
}

#[derive(Debug)]
enum State {
    Handshake,
    Status,
    Login(LoginState),
    Configuration {
        phase: ConfigurationPhase,
        authenticated_profile: GameProfile,
    },
    Play,
}

/// Semantic state for the pre-play protocol.
///
/// The broad connection protocol still selects packet IDs. This state additionally
/// owns login identity and the exact response expected from the client, preventing
/// an unauthenticated or partially configured connection from reaching play.
#[derive(Debug)]
pub(crate) struct PrePlayState {
    state: State,
}

impl PrePlayState {
    pub(crate) const fn new() -> Self {
        Self {
            state: State::Handshake,
        }
    }

    pub(crate) const fn phase(&self) -> PrePlayPhase {
        match &self.state {
            State::Handshake => PrePlayPhase::Handshake,
            State::Status => PrePlayPhase::Status,
            State::Login(LoginState::Hello) => PrePlayPhase::Login(LoginPhase::Hello),
            State::Login(LoginState::Key { .. }) => PrePlayPhase::Login(LoginPhase::Key),
            State::Login(LoginState::Authenticating) => {
                PrePlayPhase::Login(LoginPhase::Authenticating)
            }
            State::Login(LoginState::ProtocolSwitching { .. }) => {
                PrePlayPhase::Login(LoginPhase::ProtocolSwitching)
            }
            State::Configuration { phase, .. } => PrePlayPhase::Configuration(*phase),
            State::Play => PrePlayPhase::Play,
        }
    }

    pub(crate) fn select_protocol(
        &mut self,
        protocol: ConnectionProtocol,
    ) -> Result<(), PacketSequenceError> {
        if !matches!(&self.state, State::Handshake) {
            return Err(self.unexpected(PrePlayPacket::ClientIntention));
        }

        self.state = match protocol {
            ConnectionProtocol::Status => State::Status,
            ConnectionProtocol::Login => State::Login(LoginState::Hello),
            _ => return Err(self.unexpected(PrePlayPacket::ClientIntention)),
        };
        Ok(())
    }

    pub(crate) const fn expect(&self, packet: PrePlayPacket) -> Result<(), PacketSequenceError> {
        let expected = matches!(
            (&self.state, packet),
            (State::Login(LoginState::Hello), PrePlayPacket::Hello)
                | (State::Login(LoginState::Key { .. }), PrePlayPacket::Key)
                | (
                    State::Login(LoginState::ProtocolSwitching { .. }),
                    PrePlayPacket::LoginAcknowledged
                )
                | (
                    State::Configuration {
                        phase: ConfigurationPhase::SynchronizeRegistries,
                        ..
                    },
                    PrePlayPacket::SelectKnownPacks
                )
                | (
                    State::Configuration {
                        phase: ConfigurationPhase::JoinWorld,
                        ..
                    },
                    PrePlayPacket::FinishConfiguration
                )
        );
        if expected {
            Ok(())
        } else {
            Err(self.unexpected(packet))
        }
    }

    pub(crate) fn wait_for_key(
        &mut self,
        requested_username: String,
    ) -> Result<(), PacketSequenceError> {
        self.expect(PrePlayPacket::Hello)?;
        self.state = State::Login(LoginState::Key { requested_username });
        Ok(())
    }

    pub(crate) fn begin_authentication(&mut self) -> Result<String, PacketSequenceError> {
        self.expect(PrePlayPacket::Key)?;
        let State::Login(LoginState::Key { requested_username }) = &mut self.state else {
            return Err(self.unexpected(PrePlayPacket::Key));
        };
        let requested_username = mem::take(requested_username);
        self.state = State::Login(LoginState::Authenticating);
        Ok(requested_username)
    }

    pub(crate) fn complete_login(
        &mut self,
        authenticated_profile: GameProfile,
    ) -> Result<(), PacketSequenceError> {
        if !matches!(
            &self.state,
            State::Login(LoginState::Hello | LoginState::Authenticating)
        ) {
            return Err(self.unexpected(PrePlayPacket::LoginAcknowledged));
        }
        self.state = State::Login(LoginState::ProtocolSwitching {
            authenticated_profile,
        });
        Ok(())
    }

    pub(crate) fn acknowledge_login(&mut self) -> Result<(), PacketSequenceError> {
        self.expect(PrePlayPacket::LoginAcknowledged)?;
        let previous = mem::replace(&mut self.state, State::Play);
        match previous {
            State::Login(LoginState::ProtocolSwitching {
                authenticated_profile,
            }) => {
                self.state = State::Configuration {
                    phase: ConfigurationPhase::SynchronizeRegistries,
                    authenticated_profile,
                };
                Ok(())
            }
            previous => {
                self.state = previous;
                Err(self.unexpected(PrePlayPacket::LoginAcknowledged))
            }
        }
    }

    pub(crate) fn select_known_packs(&mut self) -> Result<(), PacketSequenceError> {
        self.expect(PrePlayPacket::SelectKnownPacks)?;
        let State::Configuration { phase, .. } = &mut self.state else {
            return Err(self.unexpected(PrePlayPacket::SelectKnownPacks));
        };
        *phase = ConfigurationPhase::JoinWorld;
        Ok(())
    }

    pub(crate) fn finish_configuration(&mut self) -> Result<GameProfile, PacketSequenceError> {
        self.expect(PrePlayPacket::FinishConfiguration)?;
        let previous = mem::replace(&mut self.state, State::Play);
        match previous {
            State::Configuration {
                authenticated_profile,
                ..
            } => Ok(authenticated_profile),
            previous => {
                self.state = previous;
                Err(self.unexpected(PrePlayPacket::FinishConfiguration))
            }
        }
    }

    const fn unexpected(&self, packet: PrePlayPacket) -> PacketSequenceError {
        PacketSequenceError {
            packet,
            phase: self.phase(),
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_core::player::{GameProfile, offline_uuid};
    use steel_protocol::utils::ConnectionProtocol;
    use uuid::Uuid;

    use super::{ConfigurationPhase, LoginPhase, PrePlayPacket, PrePlayPhase, PrePlayState};

    fn profile(id: Uuid, name: &str) -> GameProfile {
        GameProfile {
            id,
            name: name.to_string(),
            properties: vec![],
            profile_actions: None,
        }
    }

    fn login_state() -> PrePlayState {
        let mut state = PrePlayState::new();
        state
            .select_protocol(ConnectionProtocol::Login)
            .expect("handshake should enter login");
        state
    }

    #[test]
    fn online_flow_uses_only_the_authenticated_profile() {
        let mut state = login_state();
        let authenticated_id = Uuid::from_u128(1);

        state
            .wait_for_key("Steve".to_string())
            .expect("hello should start key exchange");
        assert_eq!(
            state
                .begin_authentication()
                .expect("key should start authentication"),
            "Steve"
        );
        state
            .complete_login(profile(authenticated_id, "Steve"))
            .expect("Mojang profile should complete login");
        state
            .acknowledge_login()
            .expect("client should acknowledge login");
        state
            .select_known_packs()
            .expect("client should select known packs");
        let accepted = state
            .finish_configuration()
            .expect("client should finish configuration");

        assert_eq!(accepted.id, authenticated_id);
        assert_eq!(state.phase(), PrePlayPhase::Play);
    }

    #[test]
    fn offline_flow_uses_the_server_derived_profile() {
        let mut state = login_state();
        let offline_id = offline_uuid("Steve");

        state
            .complete_login(profile(offline_id, "Steve"))
            .expect("offline profile should complete login after hello");
        state
            .acknowledge_login()
            .expect("client should acknowledge login");
        state
            .select_known_packs()
            .expect("client should select known packs");
        let accepted = state
            .finish_configuration()
            .expect("client should finish configuration");

        assert_eq!(accepted.id, offline_id);
        assert_eq!(state.phase(), PrePlayPhase::Play);
    }

    #[test]
    fn skipped_authentication_does_not_advance_login() {
        let mut state = login_state();

        assert!(state.acknowledge_login().is_err());
        assert_eq!(state.phase(), PrePlayPhase::Login(LoginPhase::Hello));

        state
            .wait_for_key("Steve".to_string())
            .expect("hello should start key exchange");

        assert!(state.expect(PrePlayPacket::LoginAcknowledged).is_err());
        assert!(state.acknowledge_login().is_err());
        assert_eq!(state.phase(), PrePlayPhase::Login(LoginPhase::Key));
    }

    #[test]
    fn skipped_known_pack_negotiation_does_not_finish_configuration() {
        let mut state = login_state();
        state
            .complete_login(profile(offline_uuid("Steve"), "Steve"))
            .expect("offline profile should complete login");
        state
            .acknowledge_login()
            .expect("client should acknowledge login");

        assert!(state.expect(PrePlayPacket::FinishConfiguration).is_err());
        assert!(state.finish_configuration().is_err());
        assert_eq!(
            state.phase(),
            PrePlayPhase::Configuration(ConfigurationPhase::SynchronizeRegistries)
        );
    }

    #[test]
    fn repeated_packets_do_not_advance_the_sequence() {
        let mut state = login_state();
        state
            .wait_for_key("Steve".to_string())
            .expect("hello should start key exchange");
        assert!(state.expect(PrePlayPacket::Hello).is_err());
        assert_eq!(state.phase(), PrePlayPhase::Login(LoginPhase::Key));

        assert_eq!(
            state
                .begin_authentication()
                .expect("key should start authentication"),
            "Steve"
        );
        assert!(state.begin_authentication().is_err());
        assert_eq!(
            state.phase(),
            PrePlayPhase::Login(LoginPhase::Authenticating)
        );

        state
            .complete_login(profile(Uuid::from_u128(1), "Steve"))
            .expect("authentication should complete login");
        state
            .acknowledge_login()
            .expect("client should acknowledge login");
        assert!(state.acknowledge_login().is_err());
        assert_eq!(
            state.phase(),
            PrePlayPhase::Configuration(ConfigurationPhase::SynchronizeRegistries)
        );

        state
            .select_known_packs()
            .expect("client should select known packs");
        assert!(state.select_known_packs().is_err());
        assert_eq!(
            state.phase(),
            PrePlayPhase::Configuration(ConfigurationPhase::JoinWorld)
        );

        state
            .finish_configuration()
            .expect("client should finish configuration");
        assert!(state.finish_configuration().is_err());
        assert_eq!(state.phase(), PrePlayPhase::Play);
    }
}
