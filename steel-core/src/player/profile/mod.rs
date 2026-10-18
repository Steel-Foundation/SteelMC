//! Player profile helpers for name validation, offline UUIDs, and known player lookup

mod known_players;
mod lookup;

pub(crate) use known_players::{GAME_PROFILE_CACHE_LIMIT, KnownPlayerNameLookup};
pub use known_players::{KnownPlayer, KnownPlayers};
pub use lookup::ProfileLookupError;
pub(crate) use lookup::lookup_online_profile;

pub use steel_registry::GameProfile;
use uuid::{Builder, Uuid, Variant, Version};

/// Returns whether a name is valid for online login and profile lookup.
#[must_use]
pub fn is_valid_player_name(name: &str) -> bool {
    (3..=16).contains(&name.len())
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// Generates vanilla's deterministic offline-mode UUID for a player name.
#[must_use]
pub fn offline_uuid(username: &str) -> Uuid {
    Builder::from_md5_bytes(md5::compute(format!("OfflinePlayer:{username}")).0)
        .with_version(Version::Md5)
        .with_variant(Variant::RFC4122)
        .into_uuid()
}

#[cfg(test)]
mod tests {
    use super::{is_valid_player_name, offline_uuid};

    #[test]
    fn validates_vanilla_player_names() {
        assert!(is_valid_player_name("Steve"));
        assert!(is_valid_player_name("Alex_123"));
        assert!(!is_valid_player_name("ab"));
        assert!(!is_valid_player_name("name-with-dash"));
        assert!(!is_valid_player_name("way_too_long_player_name"));
    }

    #[test]
    fn offline_uuid_matches_vanilla_name_uuid() {
        assert_eq!(
            offline_uuid("Steve").to_string(),
            "5627dd98-e6be-3c21-b8a8-e92344183641"
        );
    }
}
