//! Persisted player whitelist.
//!
//! Mirrors vanilla's `UserWhiteList` plus the server's `use-whitelist` toggle,
//! both stored together in Steel's own `whitelist.toml`.

use std::{error::Error, fmt, sync::Arc};

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use steel_utils::locks::{AsyncMutex, SyncRwLock};
use uuid::Uuid;

/// A single whitelist entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WhitelistEntry {
    /// The whitelisted player's UUID.
    pub uuid: Uuid,
    /// The whitelisted player's last-known name, for display purposes only.
    pub name: String,
}

/// Parsed `whitelist.toml` root.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WhitelistConfig {
    /// Whether the whitelist is enforced at login.
    #[serde(default)]
    pub enabled: bool,
    /// The list of whitelisted players.
    #[serde(default)]
    pub entries: Vec<WhitelistEntry>,
}

/// Persists the whitelist configuration owned outside `steel-core`.
pub trait WhitelistStore: Send + Sync {
    /// Saves the complete whitelist configuration.
    fn save(&self, config: WhitelistConfig) -> BoxFuture<'static, Result<(), WhitelistStoreError>>;

    /// Reloads the whitelist configuration from its backing storage, picking
    /// up out-of-band edits (e.g. hand-editing `whitelist.toml` while the
    /// server is running).
    fn load(&self) -> BoxFuture<'static, Result<WhitelistConfig, WhitelistStoreError>>;
}

/// Whitelist persistence failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhitelistStoreError {
    message: String,
}

impl WhitelistStoreError {
    /// Creates a persistence error from a displayable message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for WhitelistStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for WhitelistStoreError {}

/// Runtime whitelist with persistence-first updates.
pub struct WhitelistManager {
    updates: AsyncMutex<()>,
    state: SyncRwLock<WhitelistConfig>,
    store: Option<Arc<dyn WhitelistStore>>,
}

impl fmt::Debug for WhitelistManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WhitelistManager")
            .field("updates", &self.updates)
            .field("state", &self.state)
            .field("store", &self.store.as_ref().map(|_| "<whitelist store>"))
            .finish()
    }
}

impl WhitelistManager {
    /// Builds a manager from an initial config and an optional persistence store.
    #[must_use]
    pub fn new(config: WhitelistConfig, store: Option<Arc<dyn WhitelistStore>>) -> Self {
        Self {
            updates: AsyncMutex::new(()),
            state: SyncRwLock::new(config),
            store,
        }
    }

    /// Builds a manager without persistence.
    #[must_use]
    pub fn transient() -> Self {
        Self::new(WhitelistConfig::default(), None)
    }

    /// Returns whether the whitelist is currently enforced at login.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.state.read().enabled
    }

    /// Returns whether a UUID is currently whitelisted.
    #[must_use]
    pub fn is_whitelisted(&self, uuid: Uuid) -> bool {
        self.state
            .read()
            .entries
            .iter()
            .any(|entry| entry.uuid == uuid)
    }

    /// Returns every whitelisted entry.
    #[must_use]
    pub fn entries(&self) -> Vec<WhitelistEntry> {
        self.state.read().entries.clone()
    }

    /// Enables or disables the whitelist, persisting first.
    ///
    /// # Errors
    ///
    /// Returns an error when persistence fails.
    pub async fn set_enabled(&self, enabled: bool) -> Result<(), WhitelistManagerError> {
        let _guard = self.updates.lock().await;
        let mut config = self.state.read().clone();
        config.enabled = enabled;
        self.persist_locked(config).await
    }

    /// Adds a whitelist entry, persisting first.
    ///
    /// Returns whether the player was newly added (a player who's already
    /// whitelisted is left untouched, matching vanilla).
    ///
    /// # Errors
    ///
    /// Returns an error when persistence fails.
    pub async fn add(&self, entry: WhitelistEntry) -> Result<bool, WhitelistManagerError> {
        let _guard = self.updates.lock().await;
        let mut config = self.state.read().clone();
        if config
            .entries
            .iter()
            .any(|existing| existing.uuid == entry.uuid)
        {
            return Ok(false);
        }
        config.entries.push(entry);
        self.persist_locked(config).await?;
        Ok(true)
    }

    /// Removes a whitelist entry, persisting first.
    ///
    /// Returns whether an entry was actually removed.
    ///
    /// # Errors
    ///
    /// Returns an error when persistence fails.
    pub async fn remove(&self, uuid: Uuid) -> Result<bool, WhitelistManagerError> {
        let _guard = self.updates.lock().await;
        let mut config = self.state.read().clone();
        let previous_len = config.entries.len();
        config.entries.retain(|entry| entry.uuid != uuid);
        if config.entries.len() == previous_len {
            return Ok(false);
        }
        self.persist_locked(config).await?;
        Ok(true)
    }

    /// Reloads the whitelist from its backing store, discarding any
    /// in-memory state that hasn't been persisted.
    ///
    /// Does nothing when the manager has no store.
    ///
    /// # Errors
    ///
    /// Returns an error when the reload fails.
    pub async fn reload(&self) -> Result<(), WhitelistManagerError> {
        let _guard = self.updates.lock().await;
        let Some(store) = &self.store else {
            return Ok(());
        };
        let config = store.load().await?;
        *self.state.write() = config;
        Ok(())
    }

    async fn persist_locked(&self, config: WhitelistConfig) -> Result<(), WhitelistManagerError> {
        if let Some(store) = &self.store {
            store.save(config.clone()).await?;
        }
        *self.state.write() = config;
        Ok(())
    }
}

/// Whitelist manager update failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhitelistManagerError(WhitelistStoreError);

impl fmt::Display for WhitelistManagerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "failed to store whitelist: {}", self.0)
    }
}

impl Error for WhitelistManagerError {}

impl From<WhitelistStoreError> for WhitelistManagerError {
    fn from(value: WhitelistStoreError) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use futures::future::BoxFuture;
    use steel_utils::locks::SyncMutex;

    use super::{
        WhitelistConfig, WhitelistEntry, WhitelistManager, WhitelistStore, WhitelistStoreError,
    };

    #[derive(Debug)]
    struct CapturingStore {
        saved: Arc<SyncMutex<Vec<WhitelistConfig>>>,
    }

    impl WhitelistStore for CapturingStore {
        fn save(
            &self,
            config: WhitelistConfig,
        ) -> BoxFuture<'static, Result<(), WhitelistStoreError>> {
            let saved = Arc::clone(&self.saved);
            Box::pin(async move {
                saved.lock().push(config);
                Ok(())
            })
        }

        fn load(&self) -> BoxFuture<'static, Result<WhitelistConfig, WhitelistStoreError>> {
            let saved = Arc::clone(&self.saved);
            Box::pin(async move { Ok(saved.lock().last().cloned().unwrap_or_default()) })
        }
    }

    #[derive(Debug)]
    struct FailingStore;

    impl WhitelistStore for FailingStore {
        fn save(
            &self,
            _config: WhitelistConfig,
        ) -> BoxFuture<'static, Result<(), WhitelistStoreError>> {
            Box::pin(async { Err(WhitelistStoreError::new("test store failure")) })
        }

        fn load(&self) -> BoxFuture<'static, Result<WhitelistConfig, WhitelistStoreError>> {
            Box::pin(async { Err(WhitelistStoreError::new("test store failure")) })
        }
    }

    fn entry(uuid: uuid::Uuid, name: &str) -> WhitelistEntry {
        WhitelistEntry {
            uuid,
            name: name.to_owned(),
        }
    }

    #[tokio::test]
    async fn add_skips_an_already_whitelisted_player() {
        let manager = WhitelistManager::transient();
        let uuid = uuid::Uuid::nil();
        assert!(
            manager
                .add(entry(uuid, "Steve"))
                .await
                .expect("add should succeed")
        );
        assert!(
            !manager
                .add(entry(uuid, "Steve"))
                .await
                .expect("add should succeed")
        );
        assert_eq!(manager.entries().len(), 1);
    }

    #[tokio::test]
    async fn remove_reports_whether_an_entry_existed() {
        let manager = WhitelistManager::transient();
        let uuid = uuid::Uuid::nil();
        assert!(!manager.remove(uuid).await.expect("remove should succeed"));

        manager
            .add(entry(uuid, "Steve"))
            .await
            .expect("add should succeed");
        assert!(manager.remove(uuid).await.expect("remove should succeed"));
        assert!(!manager.is_whitelisted(uuid));
    }

    #[tokio::test]
    async fn set_enabled_persists_and_updates_state() {
        let manager = WhitelistManager::transient();
        assert!(!manager.is_enabled());
        manager
            .set_enabled(true)
            .await
            .expect("set_enabled should succeed");
        assert!(manager.is_enabled());
    }

    #[tokio::test]
    async fn reload_replaces_in_memory_state_from_the_store() {
        let saved = Arc::new(SyncMutex::new(Vec::new()));
        let manager = WhitelistManager::new(
            WhitelistConfig::default(),
            Some(Arc::new(CapturingStore {
                saved: Arc::clone(&saved),
            })),
        );
        let uuid = uuid::Uuid::nil();
        manager
            .add(entry(uuid, "Steve"))
            .await
            .expect("add should succeed");

        // Simulate an out-of-band edit landing in the backing store.
        saved.lock().push(WhitelistConfig {
            enabled: true,
            entries: Vec::new(),
        });

        manager.reload().await.expect("reload should succeed");
        assert!(manager.is_enabled());
        assert!(!manager.is_whitelisted(uuid));
    }

    #[tokio::test]
    async fn add_persists_before_applying_and_surfaces_store_errors() {
        let failing =
            WhitelistManager::new(WhitelistConfig::default(), Some(Arc::new(FailingStore)));
        let uuid = uuid::Uuid::nil();
        let error = failing
            .add(entry(uuid, "Steve"))
            .await
            .expect_err("store failure should surface");
        assert_eq!(
            error.to_string(),
            "failed to store whitelist: test store failure"
        );
        assert!(!failing.is_whitelisted(uuid));
    }
}
