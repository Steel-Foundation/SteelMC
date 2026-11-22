//! Player info forwarding from a proxy in front of this server.
//!
//! The login flow only talks to [`PlayerInfoForwarder`]. Each proxy protocol implements it in
//! its own module, so a protocol can move into an extension once plugins exist without
//! touching the login handlers:
//!
//! - [`velocity`]: Velocity modern forwarding, an HMAC-signed `velocity:player_info` login query.
//! - [`bungee`]: `BungeeCord` legacy forwarding in the handshake hostname, optionally checked
//!   with a `BungeeGuard` token.
//!
//! Formats and disconnect messages follow Paper/Spigot so existing proxy setups work unchanged.

pub(crate) mod bungee;
pub(crate) mod velocity;

use std::net::IpAddr;

use steel_core::{config::PlayerInfoForwarding, player::GameProfile};
use steel_protocol::packets::handshake::MAX_HOST_LENGTH;
use steel_registry::ProfileProperty;
use steel_utils::{Identifier, translations};
use text_components::TextComponent;
use uuid::Uuid;

use self::{bungee::BungeeCordForwarder, velocity::VelocityForwarder};

/// A proxy forwarding protocol, driven by the login flow.
///
/// The login flow calls, in order:
/// 1. [`max_handshake_host_length`](Self::max_handshake_host_length) and
///    [`on_handshake`](Self::on_handshake) for a login handshake,
/// 2. [`on_hello`](Self::on_hello) instead of starting encryption,
/// 3. [`on_query_answer`](Self::on_query_answer) if `on_hello` sent a [`LoginQuery`].
///
/// The login flow owns transaction ids, packet sequencing, the client address and disconnects;
/// implementations only decode and verify proxy data.
pub trait PlayerInfoForwarder: Send + Sync {
    /// Longest handshake hostname accepted, in UTF-16 code units. Defaults to vanilla's limit.
    fn max_handshake_host_length(&self) -> usize {
        MAX_HOST_LENGTH
    }

    /// Inspects the hostname of a login handshake for forwarded data.
    ///
    /// Returned data is handed to [`on_hello`](Self::on_hello), and its address replaces the
    /// client address immediately.
    ///
    /// # Errors
    /// Rejects the connection before login starts.
    fn on_handshake(&self, hostname: &str) -> Result<Option<ForwardedPlayer>, ForwardingRejection> {
        let _ = hostname;
        Ok(None)
    }

    /// Continues login after hello, with any data accepted from the handshake.
    ///
    /// # Errors
    /// Rejects the login.
    fn on_hello(
        &self,
        handshake: Option<ForwardedPlayer>,
    ) -> Result<ForwardingStep, ForwardingRejection>;

    /// Handles the answer to the [`LoginQuery`] returned by [`on_hello`](Self::on_hello).
    /// `payload` is `None` when the other side did not understand the query.
    ///
    /// # Errors
    /// Rejects the login. The default rejects like vanilla does for any unexpected answer.
    fn on_query_answer(
        &self,
        payload: Option<&[u8]>,
    ) -> Result<ForwardedPlayer, ForwardingRejection> {
        let _ = payload;
        Err(ForwardingRejection::new(
            "unexpected login query answer",
            TextComponent::translated(
                translations::MULTIPLAYER_DISCONNECT_UNEXPECTED_QUERY_RESPONSE.msg(),
            ),
        ))
    }
}

/// What the login flow does after [`PlayerInfoForwarder::on_hello`].
#[derive(Debug)]
pub enum ForwardingStep {
    /// Send a login query to the proxy and wait for its answer.
    Query(LoginQuery),
    /// Finish login with the forwarded player.
    Complete(ForwardedPlayer),
}

/// A login plugin request sent to the proxy. The login flow assigns the transaction id.
#[derive(Debug)]
pub struct LoginQuery {
    /// Channel the proxy listens on.
    pub channel: Identifier,
    /// Raw query data, sent without a length prefix.
    pub payload: Vec<u8>,
}

/// A player identity and address vouched for by the proxy.
#[derive(Debug)]
pub struct ForwardedPlayer {
    /// The client IP the proxy saw.
    pub address: IpAddr,
    /// The forwarded identity.
    pub profile: ForwardedProfile,
}

/// The forwarded identity.
#[derive(Debug)]
pub enum ForwardedProfile {
    /// The proxy forwarded the whole profile.
    Complete(GameProfile),
    /// The proxy forwarded only the UUID and properties; the name comes from the hello packet,
    /// like Spigot's `ServerLoginPacketListenerImpl.initUUID`.
    NameFromHello {
        /// The proxy's UUID for the player.
        id: Uuid,
        /// Forwarded profile properties, such as skin textures.
        properties: Vec<ProfileProperty>,
    },
}

/// Why a forwarder refused a connection.
#[derive(Debug)]
pub struct ForwardingRejection {
    /// Detail for the server log. Never sent to the client.
    pub log_reason: String,
    /// Disconnect message sent to the client.
    pub disconnect: Box<TextComponent>,
}

impl ForwardingRejection {
    /// Creates a rejection with a log detail and a client disconnect message.
    #[must_use]
    pub fn new(log_reason: impl Into<String>, disconnect: impl Into<TextComponent>) -> Self {
        Self {
            log_reason: log_reason.into(),
            disconnect: Box::new(disconnect.into()),
        }
    }
}

/// Creates the built-in forwarder for the configured mode, or `None` without a proxy.
///
/// This is the only place built-in protocols are selected; an extension would register its
/// own [`PlayerInfoForwarder`] instead.
#[must_use]
pub(crate) fn builtin_forwarder(
    mode: &PlayerInfoForwarding,
) -> Option<Box<dyn PlayerInfoForwarder>> {
    match mode {
        PlayerInfoForwarding::None => None,
        PlayerInfoForwarding::Legacy => Some(Box::new(BungeeCordForwarder::new(None))),
        PlayerInfoForwarding::BungeeGuard(secret) => {
            Some(Box::new(BungeeCordForwarder::new(Some(secret))))
        }
        PlayerInfoForwarding::Modern(secret) => {
            Some(Box::new(VelocityForwarder::new(secret.clone())))
        }
    }
}

/// Parses a forwarded client IP, the way the proxy writes it with Java's
/// `InetAddress.getHostAddress`/`getHostString`.
///
/// Unlike Spigot this never resolves a hostname: anything that is not a literal IP is rejected.
/// IPv6 brackets and zone ids (`%eth0`) are dropped because `IpAddr` cannot carry them.
fn parse_forwarded_ip(address: &str) -> Option<IpAddr> {
    let address = address
        .strip_prefix('[')
        .and_then(|address| address.strip_suffix(']'))
        .unwrap_or(address);
    let address = address.split_once('%').map_or(address, |(ip, _zone)| ip);
    address.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::parse_forwarded_ip;

    #[test]
    fn forwarded_ip_accepts_only_literal_addresses() {
        assert_eq!(
            parse_forwarded_ip("203.0.113.7"),
            Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)))
        );
        assert_eq!(
            parse_forwarded_ip("fe80::1%eth0"),
            Some(IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)))
        );
        assert_eq!(
            parse_forwarded_ip("[::1]"),
            Some(IpAddr::V6(Ipv6Addr::LOCALHOST))
        );
        // A hostname would make Spigot resolve DNS; Steel refuses it.
        assert_eq!(parse_forwarded_ip("localhost"), None);
        assert_eq!(parse_forwarded_ip(""), None);
    }
}
