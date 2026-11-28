//! `BungeeCord` legacy forwarding and `BungeeGuard`.
//!
//! With `ip_forward` enabled, `BungeeCord` (and Velocity's `legacy` mode) replaces the handshake
//! hostname with `host\0client_ip\0uuid[\0properties_json]`. Nothing authenticates this data,
//! so plain legacy forwarding is only safe when the backend is unreachable except through the
//! proxy. `BungeeGuard` adds a `bungeeguard-token` profile property that must match a shared
//! secret. Parsing follows Spigot's `ServerHandshakePacketListenerImpl`; the token check
//! follows the `BungeeGuard` plugin.

use std::net::IpAddr;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use steel_core::config::ForwardingSecret;
use steel_registry::ProfileProperty;
use steel_utils::serial::prefixed_read::MAX_STRING_LENGTH;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use super::{
    ForwardedPlayer, ForwardedProfile, ForwardingRejection, ForwardingStep, PlayerInfoForwarder,
    parse_forwarded_ip,
};

/// Profile property `BungeeGuard` uses to carry its token.
const BUNGEEGUARD_TOKEN_PROPERTY: &str = "bungeeguard-token";
/// Separator `BungeeCord` uses between the forwarded handshake fields.
const FIELD_SEPARATOR: char = '\0';
/// Spigot's raised handshake hostname limit, which leaves room for the forwarded fields.
const MAX_FORWARDED_HOST_LENGTH: usize = MAX_STRING_LENGTH;

/// SHA-256 digest of a `BungeeGuard` secret or token.
type TokenDigest = [u8; 32];

/// `BungeeCord` legacy forwarding, guarded by a `BungeeGuard` token when a secret is set.
pub(crate) struct BungeeCordForwarder {
    /// Digest of the `BungeeGuard` secret, computed once.
    bungeeguard: Option<TokenDigest>,
}

impl BungeeCordForwarder {
    pub(crate) fn new(bungeeguard: Option<&ForwardingSecret>) -> Self {
        Self {
            bungeeguard: bungeeguard.map(|secret| Sha256::digest(secret.as_bytes()).into()),
        }
    }

    fn rejection(&self, error: LegacyForwardingError) -> ForwardingRejection {
        ForwardingRejection::new(
            format!("invalid BungeeCord forwarding data ({error:?})"),
            error.disconnect_message(self.bungeeguard.is_some()),
        )
    }
}

impl PlayerInfoForwarder for BungeeCordForwarder {
    fn max_handshake_host_length(&self) -> usize {
        MAX_FORWARDED_HOST_LENGTH
    }

    fn on_handshake(&self, hostname: &str) -> Result<Option<ForwardedPlayer>, ForwardingRejection> {
        let player = LegacyForwardedPlayer::parse(hostname, self.bungeeguard.as_ref())
            .map_err(|error| self.rejection(error))?;
        Ok(Some(ForwardedPlayer {
            address: player.address,
            profile: ForwardedProfile::NameFromHello {
                id: player.id,
                properties: player.properties,
            },
        }))
    }

    fn on_hello(
        &self,
        handshake: Option<ForwardedPlayer>,
    ) -> Result<ForwardingStep, ForwardingRejection> {
        // `on_handshake` rejects every login handshake without forwarding data.
        handshake
            .map(ForwardingStep::Complete)
            .ok_or_else(|| self.rejection(LegacyForwardingError::NotForwarded))
    }
}

/// Player data forwarded in the handshake hostname.
#[derive(Debug)]
struct LegacyForwardedPlayer {
    /// The client IP the proxy saw.
    address: IpAddr,
    /// The proxy's UUID for the player: online UUID behind an online proxy, offline otherwise.
    id: Uuid,
    /// Forwarded profile properties (skin textures), with any `BungeeGuard` token removed.
    properties: Vec<ProfileProperty>,
}

/// Why handshake forwarding data was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyForwardingError {
    /// The hostname has no forwarding data: a direct connection, or `ip_forward` is off.
    NotForwarded,
    /// The forwarding fields could not be parsed.
    Malformed,
    /// `BungeeGuard` is required but the proxy sent no token.
    MissingToken,
    /// The `BungeeGuard` token is wrong or duplicated.
    InvalidToken,
}

impl LegacyForwardingError {
    /// Disconnect text, matching Spigot (legacy) and the `BungeeGuard` defaults.
    const fn disconnect_message(self, bungeeguard: bool) -> &'static str {
        match (self, bungeeguard) {
            (Self::NotForwarded | Self::Malformed, false) => {
                "If you wish to use IP forwarding, please enable it in your BungeeCord config as well!"
            }
            (Self::NotForwarded | Self::Malformed | Self::MissingToken, true)
            | (Self::MissingToken, false) => {
                "Unable to authenticate - no data was forwarded by the proxy."
            }
            (Self::InvalidToken, _) => "Unable to authenticate.",
        }
    }
}

#[derive(Deserialize)]
struct PropertyJson {
    name: String,
    value: String,
    signature: Option<String>,
}

impl LegacyForwardedPlayer {
    /// Parses the forwarding data in a login handshake hostname.
    ///
    /// With `bungeeguard` set, the forwarded properties must contain exactly one
    /// `bungeeguard-token` equal to the secret. The token is always stripped, even in plain legacy
    /// mode, so it is never sent on to other clients.
    ///
    /// # Errors
    /// See [`LegacyForwardingError`].
    fn parse(
        hostname: &str,
        bungeeguard: Option<&TokenDigest>,
    ) -> Result<Self, LegacyForwardingError> {
        // Java's `String.split` drops trailing empty fields; Spigot counts fields after that.
        let fields: Vec<&str> = hostname
            .trim_end_matches(FIELD_SEPARATOR)
            .split(FIELD_SEPARATOR)
            .collect();
        let (address, id, properties) = match fields.as_slice() {
            [_host, address, id] => (*address, *id, None),
            [_host, address, id, properties] => (*address, *id, Some(*properties)),
            _ => return Err(LegacyForwardingError::NotForwarded),
        };

        let address = parse_forwarded_ip(address).ok_or(LegacyForwardingError::Malformed)?;
        let id = Uuid::try_parse(id).map_err(|_| LegacyForwardingError::Malformed)?;
        let properties = match properties {
            Some(properties) => serde_json::from_str::<Vec<PropertyJson>>(properties)
                .map_err(|_| LegacyForwardingError::Malformed)?,
            None => Vec::new(),
        };

        let mut token = None;
        let mut forwarded = Vec::with_capacity(properties.len());
        for property in properties {
            if property.name == BUNGEEGUARD_TOKEN_PROPERTY {
                if token.replace(property.value).is_some() {
                    return Err(LegacyForwardingError::InvalidToken);
                }
                continue;
            }
            forwarded.push(
                ProfileProperty::new(property.name, property.value, property.signature)
                    .map_err(|_| LegacyForwardingError::Malformed)?,
            );
        }

        if let Some(secret) = bungeeguard {
            let token = token.ok_or(LegacyForwardingError::MissingToken)?;
            // Comparing fixed-length digests keeps the secret's length out of the timing.
            let token = Sha256::digest(token.as_bytes());
            if !bool::from(token.as_slice().ct_eq(secret)) {
                return Err(LegacyForwardingError::InvalidToken);
            }
        }

        Ok(Self {
            address,
            id,
            properties: forwarded,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    use super::{LegacyForwardedPlayer, LegacyForwardingError, TokenDigest};

    const UUID: &str = "0123456789abcdef0123456789abcdef";
    const TEXTURES: &str = r#"{"name":"textures","value":"dGV4","signature":"c2ln"}"#;

    fn token(token: &str) -> String {
        format!(r#"{{"name":"bungeeguard-token","value":"{token}"}}"#)
    }

    fn hostname(properties: Option<&str>) -> String {
        let mut hostname = format!("play.example.com\x00203.0.113.7\x00{UUID}");
        if let Some(properties) = properties {
            hostname.push('\x00');
            hostname.push_str(properties);
        }
        hostname
    }

    fn secret() -> TokenDigest {
        Sha256::digest(b"guard-token").into()
    }

    #[test]
    fn parses_bungeecord_forwarding() {
        let player = LegacyForwardedPlayer::parse(&hostname(Some(&format!("[{TEXTURES}]"))), None)
            .expect("valid legacy data");

        assert_eq!(player.address, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)));
        assert_eq!(player.id, Uuid::try_parse(UUID).expect("uuid"));
        assert_eq!(player.properties.len(), 1);
        assert_eq!(player.properties[0].name(), "textures");
        assert_eq!(player.properties[0].signature(), Some("c2ln"));

        // Without properties, and with a trailing separator Java's split would drop.
        assert!(LegacyForwardedPlayer::parse(&hostname(None), None).is_ok());
        assert!(LegacyForwardedPlayer::parse(&format!("{}\x00", hostname(None)), None).is_ok());
    }

    #[test]
    fn rejects_unforwarded_or_malformed_hostnames() {
        for direct in ["play.example.com", "play.example.com\x00203.0.113.7", ""] {
            assert_eq!(
                LegacyForwardedPlayer::parse(direct, None).err(),
                Some(LegacyForwardingError::NotForwarded)
            );
        }
        for malformed in [
            format!("host\x00example.com\x00{UUID}"),
            "host\x00203.0.113.7\x00not-a-uuid".to_owned(),
            hostname(Some("not json")),
        ] {
            assert_eq!(
                LegacyForwardedPlayer::parse(&malformed, None).err(),
                Some(LegacyForwardingError::Malformed)
            );
        }
    }

    #[test]
    fn bungeeguard_requires_exactly_one_matching_token() {
        let secret = secret();
        let valid = hostname(Some(&format!("[{TEXTURES},{}]", token("guard-token"))));
        let player = LegacyForwardedPlayer::parse(&valid, Some(&secret)).expect("valid token");
        assert!(
            player
                .properties
                .iter()
                .all(|property| property.name() != "bungeeguard-token"),
            "the token must not be forwarded to clients"
        );

        for (properties, error) in [
            (None, LegacyForwardingError::MissingToken),
            (
                Some(format!("[{TEXTURES}]")),
                LegacyForwardingError::MissingToken,
            ),
            (
                Some(format!("[{}]", token("wrong-token"))),
                LegacyForwardingError::InvalidToken,
            ),
            (
                Some(format!("[{}]", token("guard-toke"))),
                LegacyForwardingError::InvalidToken,
            ),
            (
                Some(format!(
                    "[{},{}]",
                    token("guard-token"),
                    token("guard-token")
                )),
                LegacyForwardingError::InvalidToken,
            ),
        ] {
            assert_eq!(
                LegacyForwardedPlayer::parse(&hostname(properties.as_deref()), Some(&secret)).err(),
                Some(error)
            );
        }
    }

    /// A backend in plain legacy mode behind a `BungeeGuard` proxy must not leak the token.
    #[test]
    fn legacy_mode_strips_bungeeguard_token() {
        let player =
            LegacyForwardedPlayer::parse(&hostname(Some(&format!("[{}]", token("secret")))), None)
                .expect("legacy mode ignores the token");
        assert!(player.properties.is_empty());
    }
}
