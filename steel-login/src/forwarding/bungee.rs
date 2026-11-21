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
use subtle::ConstantTimeEq;
use uuid::Uuid;

use super::parse_forwarded_ip;

/// Profile property `BungeeGuard` uses to carry its token.
const BUNGEEGUARD_TOKEN_PROPERTY: &str = "bungeeguard-token";
/// Separator `BungeeCord` uses between the forwarded handshake fields.
const FIELD_SEPARATOR: char = '\0';

/// Player data forwarded in the handshake hostname.
#[derive(Debug, Clone)]
pub(crate) struct LegacyForwardedPlayer {
    /// The client IP the proxy saw.
    pub(crate) address: IpAddr,
    /// The proxy's UUID for the player: online UUID behind an online proxy, offline otherwise.
    pub(crate) id: Uuid,
    /// Forwarded profile properties (skin textures), with any `BungeeGuard` token removed.
    pub(crate) properties: Vec<ProfileProperty>,
}

/// Why handshake forwarding data was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyForwardingError {
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
    pub(crate) const fn disconnect_message(self, bungeeguard: bool) -> &'static str {
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

/// Parses the forwarding data in a login handshake hostname.
///
/// With `bungeeguard` set, the forwarded properties must contain exactly one
/// `bungeeguard-token` equal to the secret. The token is always stripped, even in plain legacy
/// mode, so it is never sent on to other clients.
///
/// # Errors
/// See [`LegacyForwardingError`].
pub(crate) fn parse_handshake(
    hostname: &str,
    bungeeguard: Option<&ForwardingSecret>,
) -> Result<LegacyForwardedPlayer, LegacyForwardingError> {
    let mut fields: Vec<&str> = hostname.split(FIELD_SEPARATOR).collect();
    // Java's `String.split` drops trailing empty fields; Spigot counts fields after that.
    while fields.last().is_some_and(|field| field.is_empty()) {
        fields.pop();
    }
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
        let secret = Sha256::digest(secret.as_bytes());
        if !bool::from(token.as_slice().ct_eq(secret.as_slice())) {
            return Err(LegacyForwardingError::InvalidToken);
        }
    }

    Ok(LegacyForwardedPlayer {
        address,
        id,
        properties: forwarded,
    })
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use steel_core::config::ForwardingSecret;
    use uuid::Uuid;

    use super::{LegacyForwardingError, parse_handshake};

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

    fn secret() -> ForwardingSecret {
        ForwardingSecret::new(&b"guard-token"[..]).expect("non-empty secret")
    }

    #[test]
    fn parses_bungeecord_forwarding() {
        let player = parse_handshake(&hostname(Some(&format!("[{TEXTURES}]"))), None)
            .expect("valid legacy data");

        assert_eq!(player.address, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)));
        assert_eq!(player.id, Uuid::try_parse(UUID).expect("uuid"));
        assert_eq!(player.properties.len(), 1);
        assert_eq!(player.properties[0].name(), "textures");
        assert_eq!(player.properties[0].signature(), Some("c2ln"));

        // Without properties, and with a trailing separator Java's split would drop.
        assert!(parse_handshake(&hostname(None), None).is_ok());
        assert!(parse_handshake(&format!("{}\x00", hostname(None)), None).is_ok());
    }

    #[test]
    fn rejects_unforwarded_or_malformed_hostnames() {
        for direct in ["play.example.com", "play.example.com\x00203.0.113.7", ""] {
            assert_eq!(
                parse_handshake(direct, None).err(),
                Some(LegacyForwardingError::NotForwarded)
            );
        }
        for malformed in [
            format!("host\x00example.com\x00{UUID}"),
            "host\x00203.0.113.7\x00not-a-uuid".to_owned(),
            hostname(Some("not json")),
        ] {
            assert_eq!(
                parse_handshake(&malformed, None).err(),
                Some(LegacyForwardingError::Malformed)
            );
        }
    }

    #[test]
    fn bungeeguard_requires_exactly_one_matching_token() {
        let secret = secret();
        let valid = hostname(Some(&format!("[{TEXTURES},{}]", token("guard-token"))));
        let player = parse_handshake(&valid, Some(&secret)).expect("valid token");
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
                parse_handshake(&hostname(properties.as_deref()), Some(&secret)).err(),
                Some(error)
            );
        }
    }

    /// A backend in plain legacy mode behind a `BungeeGuard` proxy must not leak the token.
    #[test]
    fn legacy_mode_strips_bungeeguard_token() {
        let player = parse_handshake(&hostname(Some(&format!("[{}]", token("secret")))), None)
            .expect("legacy mode ignores the token");
        assert!(player.properties.is_empty());
    }
}
