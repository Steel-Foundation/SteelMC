//! Mojang authentication implementation.
//!
//! Handles authentication with Mojang's session servers for online mode.

use reqwest::StatusCode;
use steel_core::player::GameProfile;
use thiserror::Error;

const MOJANG_AUTH_URL: &str =
    "https://sessionserver.mojang.com/session/minecraft/hasJoined?username=";
const SERVER_ID_ARG: &str = "&serverId=";

/// An error that can occur during Mojang authentication.
#[derive(Error, Debug)]
pub enum AuthError {
    /// Authentication servers are down.
    #[error("Authentication servers are down")]
    FailedResponse,
    /// Failed to verify username.
    #[error("Failed to verify username")]
    UnverifiedUsername,
    /// You are banned from Authentication servers.
    #[error("You are banned from Authentication servers")]
    Banned,
    /// An error occurred with textures.
    #[error("Texture Error {0}")]
    TextureError(TextureError),
    /// You have disallowed actions from Authentication servers.
    #[error("You have disallowed actions from Authentication servers")]
    DisallowedAction,
    /// Failed to parse JSON into Game Profile.
    #[error("Failed to parse JSON into Game Profile")]
    FailedParse,
    /// An unknown status code was returned.
    #[error("Unknown Status Code {0}")]
    UnknownStatusCode(StatusCode),
}

/// An error that can occur with textures.
#[derive(Error, Debug)]
pub enum TextureError {
    /// Invalid URL.
    #[error("Invalid URL")]
    InvalidURL,
    /// Invalid URL scheme for player texture.
    #[error("Invalid URL scheme for player texture: {0}")]
    DisallowedUrlScheme(String),
    /// Invalid URL domain for player texture.
    #[error("Invalid URL domain for player texture: {0}")]
    DisallowedUrlDomain(String),
    /// Failed to decode base64 player texture.
    #[error("Failed to decode base64 player texture: {0}")]
    DecodeError(String),
    /// Failed to parse JSON from player texture.
    #[error("Failed to parse JSON from player texture: {0}")]
    JSONError(String),
}

const MAX_RETRIES: u32 = 3;

/// Authenticates a player with Mojang's servers.
pub async fn mojang_authenticate(
    username: &str,
    server_hash: &str,
) -> Result<GameProfile, AuthError> {
    let cap = MOJANG_AUTH_URL.len() + SERVER_ID_ARG.len() + username.len() + server_hash.len();
    let mut auth_url = String::with_capacity(cap);
    auth_url += MOJANG_AUTH_URL;
    auth_url += username;
    auth_url += SERVER_ID_ARG;
    auth_url += server_hash;

    let mut last_error = AuthError::FailedResponse;

    for _ in 0..MAX_RETRIES {
        let Ok(response) = reqwest::get(&auth_url).await else {
            last_error = AuthError::FailedResponse;
            continue;
        };

        match response.status() {
            StatusCode::OK => {
                return response.json().await.map_err(|_| AuthError::FailedParse);
            }
            StatusCode::NO_CONTENT => last_error = AuthError::UnverifiedUsername,
            other => last_error = AuthError::UnknownStatusCode(other),
        }
    }

    log::warn!("Player {username} auth failed");

    Err(last_error)
}

/// Converts a signed bytes big endian to a hex string.
///
/// Equivalent to Java's `new BigInteger(bytes).toString(16)`.
/// The first byte determines sign (two's complement). Leading zero bytes
/// are not significant for magnitude but ARE significant for sign.
#[must_use]
pub fn signed_bytes_be_to_hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "0".to_string();
    }

    // Sign is determined by the MSB of the first byte in the full array,
    // NOT after stripping leading zeros. A leading 0x00 byte means the
    // number is positive even if the next byte has its high bit set.
    let is_negative = (bytes[0] & 0x80) != 0;

    if is_negative {
        // Negative case: calculate two's complement of the full byte array.
        let mut magnitude: Vec<u8> = bytes.iter().map(|b| !*b).collect();
        for byte in magnitude.iter_mut().rev() {
            let (result, carry) = byte.overflowing_add(1);
            *byte = result;
            if !carry {
                break;
            }
        }

        let hex = hex::encode(&magnitude);
        let trimmed = hex.trim_start_matches('0');
        format!("-{trimmed}")
    } else {
        let hex = hex::encode(bytes);
        let trimmed = hex.trim_start_matches('0');
        if trimmed.is_empty() {
            "0".to_string()
        } else {
            trimmed.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    #[test]
    fn test_positive_simple() {
        // BigInteger([0x01, 0x2a]).toString(16) = "12a"
        assert_eq!(signed_bytes_be_to_hex(&[0x01, 0x2a]), "12a");
    }

    #[test]
    fn test_negative_simple() {
        // BigInteger([0xff]).toString(16) = "-1"
        assert_eq!(signed_bytes_be_to_hex(&[0xff]), "-1");
    }

    #[test]
    fn test_leading_zero_preserves_positive_sign() {
        // This was the bug: [0x00, 0x9a] is positive because the first byte is 0x00.
        // BigInteger([0x00, 0x9a]).toString(16) = "9a"
        assert_eq!(signed_bytes_be_to_hex(&[0x00, 0x9a]), "9a");
    }

    #[test]
    fn test_leading_zero_with_high_bit() {
        // BigInteger([0x00, 0xff, 0xab]).toString(16) = "ffab"
        assert_eq!(signed_bytes_be_to_hex(&[0x00, 0xff, 0xab]), "ffab");
    }

    #[test]
    fn test_negative_twos_complement() {
        // BigInteger([0x80]).toString(16) = "-80"
        assert_eq!(signed_bytes_be_to_hex(&[0x80]), "-80");
    }

    #[test]
    fn test_negative_multi_byte() {
        // BigInteger([0xfe, 0xdc]).toString(16) = "-124"
        assert_eq!(signed_bytes_be_to_hex(&[0xfe, 0xdc]), "-124");
    }

    #[test]
    fn test_zero() {
        assert_eq!(signed_bytes_be_to_hex(&[0x00]), "0");
        assert_eq!(signed_bytes_be_to_hex(&[0x00, 0x00, 0x00]), "0");
    }

    #[test]
    fn test_empty() {
        assert_eq!(signed_bytes_be_to_hex(&[]), "0");
    }

    #[test]
    fn test_known_notchian_hashes() {
        // Known test vectors from wiki.vg
        assert_eq!(
            signed_bytes_be_to_hex(&sha1::Sha1::digest(b"Notch")),
            "4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48"
        );
        assert_eq!(
            signed_bytes_be_to_hex(&sha1::Sha1::digest(b"jeb_")),
            "-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1"
        );
        assert_eq!(
            signed_bytes_be_to_hex(&sha1::Sha1::digest(b"simon")),
            "88e16a1019277b15d58faf0541e11910eb756f6"
        );
    }
}
