//! Persisted player ban list.
//!
//! Mirrors vanilla's `UserBanList`, but stored as Steel's own
//! `banned-players.toml` for consistency with the rest of Steel's
//! TOML-backed configuration rather than vanilla's `banned-players.json`.

use std::{error::Error, fmt, sync::Arc};

use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use steel_utils::{
    locks::{AsyncMutex, SyncRwLock},
    translations,
};
use text_components::{Modifier as _, TextComponent};
use uuid::Uuid;

/// A single ban entry, keyed by player UUID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BanEntry {
    /// The banned player's UUID.
    pub uuid: Uuid,
    /// The banned player's last-known name, for display purposes only.
    pub name: String,
    /// When the ban was created.
    pub created: DateTime<Utc>,
    /// Who (or what) created the ban, e.g. a command sender's name.
    pub source: String,
    /// When the ban expires. `None` means it never expires.
    #[serde(default)]
    pub expires: Option<DateTime<Utc>>,
    /// The ban reason, if any. Supports rich text (colors, hover events, ...)
    /// via SNBT, matching a plain `TextComponent::plain` for ordinary text.
    #[serde(default)]
    pub reason: Option<TextComponent>,
}

impl BanEntry {
    /// Returns whether this ban has expired and should no longer apply.
    #[must_use]
    pub fn has_expired(&self) -> bool {
        self.expires.is_some_and(|expires| expires <= Utc::now())
    }

    /// Returns vanilla's `BanListEntry.getReasonMessage`: the given reason,
    /// or a translated default when none was given.
    #[must_use]
    pub fn reason_message(&self) -> TextComponent {
        self.reason.clone().unwrap_or_else(|| {
            TextComponent::from(&translations::MULTIPLAYER_DISCONNECT_BANNED_REASON_DEFAULT)
        })
    }

    /// Builds vanilla's `PlayerList.canPlayerLogin` rejection message for this
    /// ban: the reason, with an expiration line appended when the ban isn't
    /// permanent.
    #[must_use]
    pub fn disconnect_message(&self) -> TextComponent {
        let message = translations::MULTIPLAYER_DISCONNECT_BANNED_REASON
            .message([self.reason_message()])
            .component();
        let Some(expires) = self.expires else {
            return message;
        };
        let expiration = translations::MULTIPLAYER_DISCONNECT_BANNED_EXPIRATION
            .message([TextComponent::plain(
                expires.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            )])
            .component();
        message.add_child(expiration)
    }
}

/// Parsed `banned-players.toml` root.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BanListConfig {
    /// The list of ban entries.
    #[serde(default)]
    pub bans: Vec<BanEntry>,
}

/// Persists the ban list configuration owned outside `steel-core`.
pub trait BanListStore: Send + Sync {
    /// Saves the complete ban list configuration.
    fn save_bans(&self, config: BanListConfig)
    -> BoxFuture<'static, Result<(), BanListStoreError>>;
}

/// Ban list persistence failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BanListStoreError {
    message: String,
}

impl BanListStoreError {
    /// Creates a persistence error from a displayable message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for BanListStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for BanListStoreError {}

/// Runtime ban list with persistence-first updates.
pub struct BanListManager {
    updates: AsyncMutex<()>,
    state: SyncRwLock<BanListConfig>,
    store: Option<Arc<dyn BanListStore>>,
}

impl fmt::Debug for BanListManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BanListManager")
            .field("updates", &self.updates)
            .field("state", &self.state)
            .field("store", &self.store.as_ref().map(|_| "<ban list store>"))
            .finish()
    }
}

impl BanListManager {
    /// Builds a manager from an initial config and an optional persistence store.
    #[must_use]
    pub fn new(config: BanListConfig, store: Option<Arc<dyn BanListStore>>) -> Self {
        Self {
            updates: AsyncMutex::new(()),
            state: SyncRwLock::new(config),
            store,
        }
    }

    /// Builds a manager without persistence.
    #[must_use]
    pub fn transient() -> Self {
        Self::new(BanListConfig::default(), None)
    }

    /// Returns the active (non-expired) ban entry for a UUID, if any.
    #[must_use]
    pub fn find(&self, uuid: Uuid) -> Option<BanEntry> {
        self.state
            .read()
            .bans
            .iter()
            .find(|entry| entry.uuid == uuid && !entry.has_expired())
            .cloned()
    }

    /// Returns whether a UUID currently has an active ban.
    #[must_use]
    pub fn is_banned(&self, uuid: Uuid) -> bool {
        self.find(uuid).is_some()
    }

    /// Returns all active (non-expired) ban entries.
    #[must_use]
    pub fn entries(&self) -> Vec<BanEntry> {
        self.state
            .read()
            .bans
            .iter()
            .filter(|entry| !entry.has_expired())
            .cloned()
            .collect()
    }

    /// Adds or replaces the ban entry for `entry.uuid`, persisting first.
    ///
    /// # Errors
    ///
    /// Returns an error when persistence fails.
    pub async fn add(&self, entry: BanEntry) -> Result<(), BanListManagerError> {
        let _guard = self.updates.lock().await;
        let mut config = self.state.read().clone();
        config.bans.retain(|existing| existing.uuid != entry.uuid);
        config.bans.push(entry);
        self.persist_locked(config).await
    }

    /// Removes the ban entry for a UUID, persisting first.
    ///
    /// Returns whether an entry was actually removed.
    ///
    /// # Errors
    ///
    /// Returns an error when persistence fails.
    pub async fn remove(&self, uuid: Uuid) -> Result<bool, BanListManagerError> {
        let _guard = self.updates.lock().await;
        let mut config = self.state.read().clone();
        let previous_len = config.bans.len();
        config.bans.retain(|entry| entry.uuid != uuid);
        if config.bans.len() == previous_len {
            return Ok(false);
        }
        self.persist_locked(config).await?;
        Ok(true)
    }

    async fn persist_locked(&self, config: BanListConfig) -> Result<(), BanListManagerError> {
        if let Some(store) = &self.store {
            store.save_bans(config.clone()).await?;
        }
        *self.state.write() = config;
        Ok(())
    }
}

/// Ban list manager update failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BanListManagerError(BanListStoreError);

impl fmt::Display for BanListManagerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "failed to store ban list: {}", self.0)
    }
}

impl Error for BanListManagerError {}

impl From<BanListStoreError> for BanListManagerError {
    fn from(value: BanListStoreError) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use futures::future::BoxFuture;
    use steel_utils::locks::SyncMutex;
    use uuid::Uuid;

    use super::{
        BanEntry, BanListConfig, BanListManager, BanListStore, BanListStoreError, TextComponent,
        Utc,
    };

    #[derive(Debug)]
    struct CapturingStore {
        saved: Arc<SyncMutex<Vec<BanListConfig>>>,
    }

    impl BanListStore for CapturingStore {
        fn save_bans(
            &self,
            config: BanListConfig,
        ) -> BoxFuture<'static, Result<(), BanListStoreError>> {
            let saved = Arc::clone(&self.saved);
            Box::pin(async move {
                saved.lock().push(config);
                Ok(())
            })
        }
    }

    #[derive(Debug)]
    struct FailingStore;

    impl BanListStore for FailingStore {
        fn save_bans(
            &self,
            _config: BanListConfig,
        ) -> BoxFuture<'static, Result<(), BanListStoreError>> {
            Box::pin(async { Err(BanListStoreError::new("test store failure")) })
        }
    }

    fn entry(uuid: Uuid, reason: &str) -> BanEntry {
        BanEntry {
            uuid,
            name: "Steve".to_owned(),
            created: Utc::now(),
            source: "Console".to_owned(),
            expires: None,
            reason: Some(TextComponent::plain(reason.to_owned())),
        }
    }

    #[tokio::test]
    async fn add_replaces_existing_entry_for_the_same_uuid() {
        let manager = BanListManager::transient();
        let uuid = Uuid::nil();
        manager
            .add(entry(uuid, "first"))
            .await
            .expect("add should succeed");
        manager
            .add(entry(uuid, "second"))
            .await
            .expect("add should succeed");

        let entries = manager.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].reason,
            Some(TextComponent::plain("second".to_owned()))
        );
    }

    #[tokio::test]
    async fn expired_bans_are_not_active() {
        let manager = BanListManager::transient();
        let uuid = Uuid::nil();
        let mut expired = entry(uuid, "stale");
        expired.expires = Some(Utc::now() - chrono::Duration::seconds(1));
        manager.add(expired).await.expect("add should succeed");

        assert!(!manager.is_banned(uuid));
        assert_eq!(manager.entries(), Vec::new());
    }

    #[tokio::test]
    async fn remove_reports_whether_an_entry_existed() {
        let manager = BanListManager::transient();
        let uuid = Uuid::nil();
        assert!(!manager.remove(uuid).await.expect("remove should succeed"));

        manager
            .add(entry(uuid, "reason"))
            .await
            .expect("add should succeed");
        assert!(manager.remove(uuid).await.expect("remove should succeed"));
        assert!(!manager.is_banned(uuid));
    }

    #[tokio::test]
    async fn add_persists_before_applying_and_surfaces_store_errors() {
        let saved = Arc::new(SyncMutex::new(Vec::new()));
        let manager = BanListManager::new(
            BanListConfig::default(),
            Some(Arc::new(CapturingStore {
                saved: Arc::clone(&saved),
            })),
        );
        let uuid = Uuid::nil();
        manager
            .add(entry(uuid, "reason"))
            .await
            .expect("add should succeed");
        assert_eq!(saved.lock().len(), 1);
        assert!(manager.is_banned(uuid));

        let failing = BanListManager::new(BanListConfig::default(), Some(Arc::new(FailingStore)));
        let error = failing
            .add(entry(uuid, "reason"))
            .await
            .expect_err("store failure should surface");
        assert_eq!(
            error.to_string(),
            "failed to store ban list: test store failure"
        );
        assert!(!failing.is_banned(uuid));
    }
}
