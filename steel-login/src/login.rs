//! Login helper functions.
//!
//! Contains utilities for player name validation and offline UUID generation.

use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Checks if a player name is valid.
///
/// A valid player name:
/// - Is between 3 and 16 characters long
/// - Contains only ASCII alphanumeric characters or underscores
#[must_use]
pub fn is_valid_player_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Generates an offline mode UUID for a player.
///
/// This creates a deterministic UUID based on the username hash,
/// used when the server is in offline mode.
///
/// # Errors
/// Returns an error if the UUID cannot be created from the hash bytes.
pub fn offline_uuid(username: &str) -> Result<Uuid, uuid::Error> {
    Uuid::from_slice(&Sha256::digest(username)[..16])
}
