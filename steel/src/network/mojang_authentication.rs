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
            StatusCode::NO_CONTENT => return Err(AuthError::UnverifiedUsername),
            other => last_error = AuthError::UnknownStatusCode(other),
        }
    }

    log::warn!("Plauer {username} auth failed");

    Err(last_error)
}

/// Converts a signed bytes big endian to a hex string.
#[must_use]
pub fn signed_bytes_be_to_hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "0".to_string();
    }

    // Find the first non-zero byte to handle cases like `[0x00, 0x1a]`
    let first_digit = bytes.iter().position(|&b| b != 0);

    // If all bytes are zero, the number is 0.
    let Some(start_index) = first_digit else {
        return "0".to_string();
    };

    let significant_bytes = &bytes[start_index..];
    let is_negative = (significant_bytes[0] & 0x80) != 0;

    if is_negative {
        // Negative case: calculate two's complement of the original full byte array
        // to preserve the correct number of bits for the calculation.
        let mut inverted_bytes: Vec<u8> = bytes.iter().map(|b| !*b).collect();
        for byte in inverted_bytes.iter_mut().rev() {
            let (result, carry) = byte.overflowing_add(1);
            *byte = result;
            if !carry {
                break;
            }
        }

        // Now, find the first significant digit of the *result*
        let mag_start_index = inverted_bytes.iter().position(|&b| b != 0).unwrap_or(0);

        // Format the result with a leading '-'
        format!("-{}", hex::encode(&inverted_bytes[mag_start_index..]))
    } else {
        // Positive case: just encode the significant bytes.
        hex::encode(significant_bytes)
    }
}
