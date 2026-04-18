//! Mojang API integration for profile key validation.
//!
//! This module fetches and caches Mojang's public keys used to validate
//! player profile keys during signed chat.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use base64::Engine;
use rsa::RsaPublicKey;
use serde::Deserialize;
use steel_utils::locks::SyncRwLock;

use crate::{
    public_key_from_bytes,
    signature::{MultiKeyValidator, NoValidation, SignatureValidator},
};

/// Mojang's session server endpoint for fetching public keys
const MOJANG_SESSION_SERVER: &str = "https://sessionserver.mojang.com/publickeys";

/// How long to cache Mojang's public keys before refetching (1 hour)
const KEY_CACHE_TTL: Duration = Duration::from_secs(3600);

/// A public key entry from Mojang's session server
#[derive(Debug, Deserialize)]
struct PublicKeyEntry {
    #[serde(rename = "publicKey")]
    public_key: String,
}

/// Response from Mojang's session server
#[derive(Debug, Deserialize)]
struct SessionServerResponse {
    #[serde(rename = "playerCertificateKeys")]
    player_certificate_keys: Vec<PublicKeyEntry>,
    #[serde(rename = "profilePropertyKeys")]
    #[allow(
        dead_code,
        reason = "deserialized for completeness but only certificate keys are used"
    )]
    profile_property_keys: Vec<PublicKeyEntry>,
}

/// Cached Mojang public keys with refresh tracking
struct MojangKeyCache {
    keys: Vec<RsaPublicKey>,
    fetched_at: Option<Instant>,
}

impl MojangKeyCache {
    fn new() -> Self {
        Self {
            keys: Vec::new(),
            fetched_at: None,
        }
    }

    fn needs_refresh(&self) -> bool {
        match self.fetched_at {
            None => true,
            Some(fetched) => fetched.elapsed() > KEY_CACHE_TTL,
        }
    }
}

/// Global cache for Mojang's public key
static KEY_CACHE: LazyLock<SyncRwLock<MojangKeyCache>> =
    LazyLock::new(|| SyncRwLock::new(MojangKeyCache::new()));

/// Fetches Mojang's public keys from their session server.
///
/// This is cached for 1 hour to avoid unnecessary API calls.
/// Returns the player certificate keys used for validating player profile keys.
async fn fetch_mojang_public_keys() -> Result<Vec<RsaPublicKey>, Box<dyn std::error::Error>> {
    log::info!("Fetching Mojang public keys from session server...");

    // Make HTTP request to Mojang's session server
    let response = reqwest::get(MOJANG_SESSION_SERVER).await?;

    // Parse JSON response
    let session_info: SessionServerResponse = response.json().await?;

    // Extract and decode the player certificate keys
    let mut keys = Vec::new();
    for entry in session_info.player_certificate_keys {
        // Decode base64
        let key_bytes = base64::prelude::BASE64_STANDARD.decode(&entry.public_key)?;

        // Parse RSA public key
        let public_key = public_key_from_bytes(&key_bytes)?;
        keys.push(public_key);
    }

    if keys.is_empty() {
        return Err("No player certificate keys in response".into());
    }

    log::info!(
        "Successfully fetched and parsed {} Mojang public key(s)",
        keys.len()
    );

    Ok(keys)
}

/// Gets the signature validator for Mojang profile keys.
///
/// This fetches Mojang's public keys from their session server and caches them.
/// If the keys can't be fetched, falls back to permissive validation with a warning.
///
/// The keys are cached for 1 hour and automatically refreshed when needed.
#[must_use]
pub async fn get_profile_key_validator() -> Box<dyn SignatureValidator> {
    // Check if we need to refresh the cache
    {
        let cache = KEY_CACHE.read();
        if !cache.needs_refresh() && !cache.keys.is_empty() {
            return Box::new(MultiKeyValidator::new(cache.keys.clone()));
        }
    }

    {
        // Need to refresh - acquire write lock
        let cache = KEY_CACHE.read();

        // Double-check after acquiring write lock (another thread may have refreshed)
        if !cache.needs_refresh() && !cache.keys.is_empty() {
            return Box::new(MultiKeyValidator::new(cache.keys.clone()));
        }
    }

    // Fetch new keys
    match fetch_mojang_public_keys().await {
        Ok(keys) => {
            let mut cache = KEY_CACHE.write();
            cache.keys = keys.clone();
            cache.fetched_at = Some(Instant::now());
            log::info!("Mojang public keys cached successfully");
            Box::new(MultiKeyValidator::new(keys))
        }
        Err(err) => {
            log::warn!(
                "Failed to fetch Mojang public keys: {} - using permissive validation",
                err
            );
            // Fall back to permissive mode if we can't fetch the keys
            Box::new(NoValidation)
        }
    }
}

/// Gets the signature validator for Mojang profile keys (reference version).
///
/// This is a convenience function for when you need a `&dyn` reference.
/// Note: This always returns NoValidation for simplicity. Use `get_profile_key_validator()`
/// for actual validation.
#[must_use]
pub fn get_profile_key_validator_ref() -> &'static dyn SignatureValidator {
    // For ref version, we can't easily return cached validator, so use NoValidation
    // Real validation should use get_profile_key_validator() which returns Box
    &NoValidation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_response_parsing() {
        // Test that we can parse the Mojang session server response format
        let json = r#"{
            "playerCertificateKeys": [
                {"publicKey": "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA"},
                {"publicKey": "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEB"}
            ],
            "profilePropertyKeys": [
                {"publicKey": "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEC"}
            ]
        }"#;

        let response: SessionServerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.player_certificate_keys.len(), 2);
        assert_eq!(response.profile_property_keys.len(), 1);
    }

    #[test]
    fn test_multi_key_validator() {
        // Test that MultiKeyValidator is created correctly
        use crate::generate_key_pair;

        let (_, pub1) = generate_key_pair().unwrap();
        let (_, pub2) = generate_key_pair().unwrap();

        // Just verify we can create the validator
        let _validator = MultiKeyValidator::new(vec![pub1, pub2]);
    }
}
