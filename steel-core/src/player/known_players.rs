//! Player profiles known to this server.

use chrono::{Months, Utc};
use uuid::Uuid;

pub(crate) const GAME_PROFILE_CACHE_LIMIT: usize = 1_000;

/// One cached player identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownPlayer {
    uuid: Uuid,
    last_known_name: String,
    expires_at_millis: i64,
}

impl KnownPlayer {
    /// Creates a cached player identity.
    ///
    /// Steel persists the expiration instant directly in UTC instead of
    /// vanilla's locale-formatted date, while retaining its one-month lifetime.
    #[must_use]
    pub fn new(uuid: Uuid, last_known_name: impl Into<String>) -> Self {
        let now = Utc::now();
        let Some(expiration) = now.checked_add_months(Months::new(1)) else {
            unreachable!("current UTC time plus one month must fit chrono's date range");
        };
        Self {
            uuid,
            last_known_name: last_known_name.into(),
            expires_at_millis: expiration.timestamp_millis(),
        }
    }

    pub(crate) fn with_expiration(
        uuid: Uuid,
        last_known_name: impl Into<String>,
        expires_at_millis: i64,
    ) -> Self {
        Self {
            uuid,
            last_known_name: last_known_name.into(),
            expires_at_millis,
        }
    }

    /// Returns the player's UUID.
    #[must_use]
    pub const fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Returns the player's last observed profile name.
    #[must_use]
    pub fn last_known_name(&self) -> &str {
        &self.last_known_name
    }

    pub(crate) const fn expires_at_millis(&self) -> i64 {
        self.expires_at_millis
    }
}

/// In-memory player identity cache.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KnownPlayers {
    entries: Vec<KnownPlayer>,
}

impl KnownPlayers {
    /// Creates an empty cache.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Creates a normalized cache from entries.
    #[must_use]
    pub fn from_entries(entries: impl IntoIterator<Item = KnownPlayer>) -> Self {
        let mut players = Self::new();
        for entry in entries {
            players.insert_loaded(entry);
        }
        players
    }

    /// Returns entries in stable cache order.
    #[must_use]
    pub fn entries(&self) -> &[KnownPlayer] {
        &self.entries
    }

    /// Records the latest identity for a UUID and case-insensitive name.
    ///
    /// A name can belong to only one UUID. Returns whether the cache changed.
    pub fn record(&mut self, uuid: Uuid, last_known_name: impl Into<String>) -> bool {
        let last_known_name = last_known_name.into();
        if let Some(index) = self.entries.iter().position(|entry| entry.uuid == uuid) {
            self.entries.remove(index);
        }
        let duplicate_name = self
            .entries
            .iter()
            .position(|entry| entry.last_known_name.eq_ignore_ascii_case(&last_known_name));
        if let Some(index) = duplicate_name {
            self.entries.remove(index);
        }

        self.entries
            .insert(0, KnownPlayer::new(uuid, last_known_name));
        true
    }

    /// Resolves and touches a cached name, removing it when its vanilla expiry elapsed.
    pub(crate) fn resolve_name(&mut self, name: &str, now_millis: i64) -> KnownPlayerNameLookup {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.last_known_name.eq_ignore_ascii_case(name))
        else {
            return KnownPlayerNameLookup::Missing;
        };
        let entry = self.entries.remove(index);
        if now_millis >= entry.expires_at_millis {
            return KnownPlayerNameLookup::Expired;
        }
        self.entries.insert(0, entry.clone());
        KnownPlayerNameLookup::Found(entry)
    }

    /// Resolves and touches a cached UUID. Vanilla does not expiry-check UUID lookups.
    pub(crate) fn resolve_uuid(&mut self, uuid: Uuid) -> Option<KnownPlayer> {
        let index = self.entries.iter().position(|entry| entry.uuid == uuid)?;
        let entry = self.entries.remove(index);
        self.entries.insert(0, entry.clone());
        Some(entry)
    }

    /// Looks up a profile by UUID.
    #[must_use]
    pub fn by_uuid(&self, uuid: Uuid) -> Option<&KnownPlayer> {
        self.entries.iter().find(|entry| entry.uuid == uuid)
    }

    /// Looks up a profile by case-insensitive name.
    #[must_use]
    pub fn by_name(&self, name: &str) -> Option<&KnownPlayer> {
        self.entries
            .iter()
            .find(|entry| entry.last_known_name.eq_ignore_ascii_case(name))
    }

    fn insert_loaded(&mut self, entry: KnownPlayer) {
        if self.entries.iter().any(|current| {
            current.uuid == entry.uuid
                || current
                    .last_known_name
                    .eq_ignore_ascii_case(&entry.last_known_name)
        }) {
            return;
        }
        self.entries.push(entry);
    }
}

pub(crate) enum KnownPlayerNameLookup {
    Found(KnownPlayer),
    Expired,
    Missing,
}

#[cfg(test)]
mod tests {
    use super::{KnownPlayer, KnownPlayers};
    use chrono::{Months, Utc};
    use uuid::Uuid;

    #[test]
    fn record_updates_an_existing_uuid_name() {
        let uuid = Uuid::from_u128(1);
        let mut players = KnownPlayers::new();

        assert!(players.record(uuid, "Steve"));
        assert!(players.record(uuid, "Alex"));
        assert!(players.record(uuid, "Alex"));
        assert_eq!(players.entries().len(), 1);
        assert_eq!(
            players.by_uuid(uuid).map(KnownPlayer::last_known_name),
            Some("Alex")
        );
        assert!(players.by_name("alex").is_some());
    }

    #[test]
    fn record_reassigns_a_duplicate_name_to_the_latest_uuid() {
        let old_uuid = Uuid::from_u128(1);
        let new_uuid = Uuid::from_u128(2);
        let mut players = KnownPlayers::from_entries([
            KnownPlayer::new(old_uuid, "Steve"),
            KnownPlayer::new(Uuid::from_u128(3), "Alex"),
        ]);

        assert!(players.record(new_uuid, "steve"));
        assert!(players.by_uuid(old_uuid).is_none());
        assert_eq!(
            players.by_name("STEVE").map(KnownPlayer::uuid),
            Some(new_uuid)
        );
        assert_eq!(players.entries().len(), 2);
    }

    #[test]
    fn record_renews_an_unchanged_identity() {
        let uuid = Uuid::from_u128(1);
        let old_expiration = Utc::now()
            .checked_add_months(Months::new(1))
            .expect("test timestamp plus one month should fit")
            .timestamp_millis()
            - 1;
        let mut players = KnownPlayers::from_entries([KnownPlayer::with_expiration(
            uuid,
            "Steve",
            old_expiration,
        )]);

        assert!(players.record(uuid, "Steve"));
        assert!(
            players
                .by_uuid(uuid)
                .is_some_and(|player| player.expires_at_millis() > old_expiration)
        );
    }

    #[test]
    fn name_resolution_expires_and_removes_stale_entries() {
        let uuid = Uuid::from_u128(1);
        let mut players =
            KnownPlayers::from_entries([KnownPlayer::with_expiration(uuid, "Steve", 10)]);

        assert!(matches!(
            players.resolve_name("Steve", 9),
            super::KnownPlayerNameLookup::Found(profile) if profile.uuid() == uuid
        ));
        assert!(matches!(
            players.resolve_name("Steve", 10),
            super::KnownPlayerNameLookup::Expired
        ));
        assert!(players.entries().is_empty());
    }
}
