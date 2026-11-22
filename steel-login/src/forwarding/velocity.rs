//! Velocity modern forwarding.
//!
//! The server sends a `velocity:player_info` login query carrying the highest forwarding
//! version it understands. Velocity answers with `HMAC-SHA256(secret, data) || data`, where
//! `data` is the forwarding version, the client IP and the authenticated game profile.
//! Format and limits follow Paper's `VelocityProxy`.

use std::{
    io::{self, Cursor},
    net::IpAddr,
};

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use steel_core::{config::ForwardingSecret, player::GameProfile};
use steel_utils::{
    Identifier,
    codec::VarInt,
    serial::{
        ReadFrom,
        prefixed_read::{MAX_STRING_LENGTH, read_utf},
    },
};
use thiserror::Error;

use super::{
    ForwardedPlayer, ForwardedProfile, ForwardingRejection, ForwardingStep, LoginQuery,
    PlayerInfoForwarder, parse_forwarded_ip,
};

/// Login query channel Velocity answers with forwarded player info.
const PLAYER_INFO_CHANNEL: Identifier = Identifier::new_static("velocity", "player_info");

/// Velocity `MODERN_DEFAULT`, the first forwarding version.
const MODERN_DEFAULT: u8 = 1;
/// Velocity `MODERN_LAZY_SESSION`. Versions 2 and 3 append a 1.19-1.19.2 player key, which
/// this protocol version has no use for, so it is left unread like Paper does.
const MODERN_LAZY_SESSION: u8 = 4;
/// Highest forwarding version this server requests and accepts.
const MAX_SUPPORTED_FORWARDING_VERSION: u8 = MODERN_LAZY_SESSION;
/// Length of the HMAC-SHA256 signature prefix, in bytes.
const SIGNATURE_LEN: usize = 32;

/// Kick message for a client that answered the query without forwarding data, i.e. one that
/// connected directly instead of through Velocity. Paper's wording.
const DIRECT_CONNECTION_MESSAGE: &str = "This server requires you to connect with Velocity.";
/// Kick message for forwarding data that fails verification or decoding. Paper's wording for
/// a bad signature.
const VERIFICATION_FAILURE_MESSAGE: &str = "Unable to verify player details";

/// Velocity modern forwarding, verified with the shared forwarding secret.
pub(crate) struct VelocityForwarder {
    secret: ForwardingSecret,
}

impl VelocityForwarder {
    pub(crate) const fn new(secret: ForwardingSecret) -> Self {
        Self { secret }
    }
}

impl PlayerInfoForwarder for VelocityForwarder {
    /// Sends the `velocity:player_info` query instead of the encryption request, like Paper.
    fn on_hello(
        &self,
        _handshake: Option<ForwardedPlayer>,
    ) -> Result<ForwardingStep, ForwardingRejection> {
        Ok(ForwardingStep::Query(LoginQuery {
            channel: PLAYER_INFO_CHANNEL,
            payload: vec![MAX_SUPPORTED_FORWARDING_VERSION],
        }))
    }

    fn on_query_answer(
        &self,
        payload: Option<&[u8]>,
    ) -> Result<ForwardedPlayer, ForwardingRejection> {
        // A vanilla client answers unknown queries without data: it bypassed the proxy.
        let Some(payload) = payload else {
            return Err(ForwardingRejection::new(
                "connected without Velocity forwarding",
                DIRECT_CONNECTION_MESSAGE,
            ));
        };
        // The detail is only logged; the client gets Paper's generic message.
        let info = VelocityPlayerInfo::read(&self.secret, payload).map_err(|error| {
            ForwardingRejection::new(error.to_string(), VERIFICATION_FAILURE_MESSAGE)
        })?;
        Ok(ForwardedPlayer {
            address: info.address,
            profile: ForwardedProfile::Complete(info.profile),
        })
    }
}

/// Player info forwarded by Velocity, after its signature verified.
#[derive(Debug)]
struct VelocityPlayerInfo {
    address: IpAddr,
    profile: GameProfile,
}

/// Why a Velocity forwarding payload was rejected.
#[derive(Debug, Error)]
enum VelocityForwardingError {
    /// Wrong secret, tampered payload, or a payload not produced by Velocity.
    #[error("forwarding signature did not verify")]
    Integrity,
    /// The proxy speaks a forwarding version this server did not ask for.
    #[error("unsupported forwarding version {0}, wanted up to {MAX_SUPPORTED_FORWARDING_VERSION}")]
    UnsupportedVersion(i32),
    /// The forwarded client address is not a literal IP.
    #[error("forwarded address is not an IP address")]
    InvalidAddress,
    /// The signed payload could not be decoded.
    #[error("malformed forwarding data: {0}")]
    Malformed(#[from] io::Error),
}

impl VelocityPlayerInfo {
    /// Verifies and decodes a `velocity:player_info` answer.
    ///
    /// # Errors
    /// Returns [`VelocityForwardingError::Integrity`] unless the signature matches `secret`; nothing
    /// after the signature is parsed before that check passes.
    fn read(secret: &ForwardingSecret, payload: &[u8]) -> Result<Self, VelocityForwardingError> {
        let data = Self::verify(secret, payload)?;
        let data = &mut Cursor::new(data);

        let version = VarInt::read(data)?.0;
        let supported = u8::try_from(version).is_ok_and(|version| {
            (MODERN_DEFAULT..=MAX_SUPPORTED_FORWARDING_VERSION).contains(&version)
        });
        if !supported {
            return Err(VelocityForwardingError::UnsupportedVersion(version));
        }
        // Paper reads the address with the default `readUtf()` bound.
        let address = parse_forwarded_ip(&read_utf(data, MAX_STRING_LENGTH)?)
            .ok_or(VelocityForwardingError::InvalidAddress)?;
        let profile = GameProfile::read(data)?;
        Ok(Self { address, profile })
    }

    /// Checks the HMAC-SHA256 signature in constant time and returns the signed data.
    fn verify<'a>(
        secret: &ForwardingSecret,
        payload: &'a [u8],
    ) -> Result<&'a [u8], VelocityForwardingError> {
        let (signature, data) = payload
            .split_at_checked(SIGNATURE_LEN)
            .ok_or(VelocityForwardingError::Integrity)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map_err(|_| VelocityForwardingError::Integrity)?;
        mac.update(data);
        mac.verify_slice(signature)
            .map_err(|_| VelocityForwardingError::Integrity)?;
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;
    use steel_core::{
        config::ForwardingSecret,
        player::{GameProfile, offline_uuid},
    };
    use steel_registry::ProfileProperty;
    use steel_utils::{
        codec::VarInt,
        serial::{PrefixedWrite, WriteTo},
    };
    use uuid::Uuid;

    use super::{VelocityForwardingError, VelocityPlayerInfo};

    const SECRET: &[u8] = b"velocity-secret";

    fn secret() -> ForwardingSecret {
        ForwardingSecret::new(SECRET).expect("non-empty secret")
    }

    fn profile() -> GameProfile {
        GameProfile::new(
            Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef),
            "Steve".to_owned(),
            vec![
                ProfileProperty::new(
                    "textures".to_owned(),
                    "dGV4dHVyZXM=".to_owned(),
                    Some("c2lnbmF0dXJl".to_owned()),
                )
                .expect("valid property"),
            ],
        )
        .expect("valid profile")
    }

    /// Builds the payload the way Velocity's `createForwardingData` does.
    fn forwarding_data(version: i32, address: &str, profile: &GameProfile) -> Vec<u8> {
        let mut data = Vec::new();
        VarInt(version).write(&mut data).expect("write version");
        address
            .write_prefixed::<VarInt>(&mut data)
            .expect("write address");
        profile.write(&mut data).expect("write profile");
        data
    }

    fn sign(secret: &[u8], data: &[u8]) -> Vec<u8> {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("any key size");
        mac.update(data);
        let mut payload = mac.finalize().into_bytes().to_vec();
        payload.extend_from_slice(data);
        payload
    }

    #[test]
    fn accepts_signed_forwarding_data() {
        let profile = profile();
        let payload = sign(SECRET, &forwarding_data(4, "203.0.113.7", &profile));

        let info = VelocityPlayerInfo::read(&secret(), &payload).expect("valid payload");

        assert_eq!(info.address, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)));
        assert_eq!(info.profile, profile);
    }

    /// Versions 2 and 3 append a player key after the profile; it is ignored, not rejected.
    #[test]
    fn ignores_trailing_player_key_of_keyed_versions() {
        let profile = profile();
        let mut data = forwarding_data(3, "203.0.113.7", &profile);
        data.extend_from_slice(&[0xAA; 64]);

        let info =
            VelocityPlayerInfo::read(&secret(), &sign(SECRET, &data)).expect("valid payload");
        assert_eq!(info.profile, profile);
    }

    #[test]
    fn rejects_wrong_secret_and_tampering() {
        let profile = profile();
        let data = forwarding_data(4, "203.0.113.7", &profile);

        let wrong_secret = sign(b"another-secret", &data);
        assert!(matches!(
            VelocityPlayerInfo::read(&secret(), &wrong_secret),
            Err(VelocityForwardingError::Integrity)
        ));

        // Swap in another identity after signing.
        let mut tampered = sign(SECRET, &data);
        let forged = forwarding_data(
            4,
            "203.0.113.7",
            &GameProfile::new(offline_uuid("Notch"), "Notch".to_owned(), Vec::new())
                .expect("valid profile"),
        );
        tampered.truncate(32);
        tampered.extend_from_slice(&forged);
        assert!(matches!(
            VelocityPlayerInfo::read(&secret(), &tampered),
            Err(VelocityForwardingError::Integrity)
        ));

        for short in [&[][..], &[0; 31][..], &[0; 32][..]] {
            assert!(matches!(
                VelocityPlayerInfo::read(&secret(), short),
                Err(VelocityForwardingError::Integrity)
            ));
        }
    }

    #[test]
    fn rejects_unrequested_versions() {
        let profile = profile();
        for version in [0, 5] {
            let payload = sign(SECRET, &forwarding_data(version, "203.0.113.7", &profile));
            assert!(matches!(
                VelocityPlayerInfo::read(&secret(), &payload),
                Err(VelocityForwardingError::UnsupportedVersion(v)) if v == version
            ));
        }
    }

    #[test]
    fn rejects_hostname_addresses() {
        let payload = sign(SECRET, &forwarding_data(4, "example.com", &profile()));
        assert!(matches!(
            VelocityPlayerInfo::read(&secret(), &payload),
            Err(VelocityForwardingError::InvalidAddress)
        ));
    }
}
