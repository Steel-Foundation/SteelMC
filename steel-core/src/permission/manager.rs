use std::{error::Error, fmt, sync::Arc};

use futures::future::BoxFuture;
use steel_utils::locks::{AsyncMutex, SyncRwLock};

use super::{
    PermissionConfigError, PermissionGroups, PermissionGroupsConfig, PermissionMetadataSet,
    PermissionSet,
};

/// Persists permission group configuration owned outside `steel-core`.
pub trait PermissionGroupStore: Send + Sync {
    /// Saves the complete permission group configuration.
    fn save_groups(
        &self,
        config: PermissionGroupsConfig,
    ) -> BoxFuture<'static, Result<(), PermissionGroupStoreError>>;
}

/// Permission group persistence failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionGroupStoreError {
    message: String,
}

impl PermissionGroupStoreError {
    /// Creates a persistence error from a displayable message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for PermissionGroupStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for PermissionGroupStoreError {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PermissionGroupManagerState {
    config: PermissionGroupsConfig,
    groups: PermissionGroups,
}

/// Runtime permission groups with serialized, persistence-first updates.
pub struct PermissionGroupManager {
    updates: AsyncMutex<()>,
    state: SyncRwLock<PermissionGroupManagerState>,
    store: Option<Arc<dyn PermissionGroupStore>>,
}

impl fmt::Debug for PermissionGroupManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PermissionGroupManager")
            .field("updates", &self.updates)
            .field("state", &self.state)
            .field("store", &self.store.as_ref().map(|_| "<group store>"))
            .finish()
    }
}

impl PermissionGroupManager {
    /// Builds a manager from typed config and an optional persistence store.
    ///
    /// # Errors
    ///
    /// Returns an error when the initial config does not resolve.
    pub fn new(
        config: PermissionGroupsConfig,
        store: Option<Arc<dyn PermissionGroupStore>>,
    ) -> Result<Self, PermissionConfigError> {
        let groups = PermissionGroups::from_config(config.clone())?;
        Ok(Self {
            updates: AsyncMutex::new(()),
            state: SyncRwLock::new(PermissionGroupManagerState { config, groups }),
            store,
        })
    }

    /// Builds a manager without persistence.
    ///
    /// # Errors
    ///
    /// Returns an error when the initial config does not resolve.
    pub fn transient(config: PermissionGroupsConfig) -> Result<Self, PermissionConfigError> {
        Self::new(config, None)
    }

    /// Returns a typed snapshot of the current config.
    #[must_use]
    pub fn config_snapshot(&self) -> PermissionGroupsConfig {
        self.state.read().config.clone()
    }

    /// Returns whether a configured group exists.
    #[must_use]
    pub fn contains_group(&self, group: &str) -> bool {
        self.state.read().groups.contains_group(group)
    }

    /// Returns configured group names sorted by name.
    #[must_use]
    pub fn group_names(&self) -> Vec<String> {
        self.state.read().groups.groups().keys().cloned().collect()
    }

    /// Builds an effective permission set from the current group snapshot.
    #[must_use]
    pub fn effective_permissions(
        &self,
        assigned_groups: &[String],
        subject_permissions: &PermissionSet,
    ) -> PermissionSet {
        self.state
            .read()
            .groups
            .effective_permissions(assigned_groups, subject_permissions)
    }

    /// Builds effective metadata from the current group snapshot.
    #[must_use]
    pub fn effective_metadata(
        &self,
        assigned_groups: &[String],
        subject_metadata: &PermissionMetadataSet,
    ) -> PermissionMetadataSet {
        self.state
            .read()
            .groups
            .effective_metadata(assigned_groups, subject_metadata)
    }

    /// Replaces the complete config after validation and optional persistence.
    ///
    /// # Errors
    ///
    /// Returns an error when validation or persistence fails.
    pub async fn replace_config(
        &self,
        config: PermissionGroupsConfig,
    ) -> Result<(), PermissionGroupManagerError> {
        let _guard = self.updates.lock().await;
        self.replace_config_locked(config).await
    }

    /// Updates the latest config under the manager update lock.
    ///
    /// # Errors
    ///
    /// Returns an error when the updated config is invalid or cannot be persisted.
    pub async fn update_config(
        &self,
        update: impl FnOnce(&mut PermissionGroupsConfig) + Send,
    ) -> Result<(), PermissionGroupManagerError> {
        let _guard = self.updates.lock().await;
        let mut config = self.state.read().config.clone();
        update(&mut config);
        self.replace_config_locked(config).await
    }

    /// Updates the latest config with a fallible caller-owned edit.
    ///
    /// # Errors
    ///
    /// Returns the edit error, or a validation or persistence error.
    pub async fn try_update_config<T, E>(
        &self,
        update: impl FnOnce(&mut PermissionGroupsConfig) -> Result<T, E> + Send,
    ) -> Result<T, PermissionGroupUpdateError<E>>
    where
        T: Send,
        E: Send,
    {
        let _guard = self.updates.lock().await;
        let current = self.state.read().config.clone();
        let mut config = current.clone();
        let result = update(&mut config).map_err(PermissionGroupUpdateError::Edit)?;
        if config == current {
            return Ok(result);
        }

        self.replace_config_locked(config)
            .await
            .map_err(PermissionGroupUpdateError::Manager)?;
        Ok(result)
    }

    async fn replace_config_locked(
        &self,
        config: PermissionGroupsConfig,
    ) -> Result<(), PermissionGroupManagerError> {
        let groups = PermissionGroups::from_config(config.clone())?;
        if let Some(store) = &self.store {
            store.save_groups(config.clone()).await?;
        }
        *self.state.write() = PermissionGroupManagerState { config, groups };
        Ok(())
    }
}

/// Permission group manager update failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionGroupManagerError {
    /// The candidate config is invalid.
    Config(PermissionConfigError),
    /// The candidate config could not be persisted.
    Store(PermissionGroupStoreError),
}

impl fmt::Display for PermissionGroupManagerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(formatter, "invalid permission groups config: {error}"),
            Self::Store(error) => write!(formatter, "failed to store permission groups: {error}"),
        }
    }
}

impl Error for PermissionGroupManagerError {}

impl From<PermissionConfigError> for PermissionGroupManagerError {
    fn from(value: PermissionConfigError) -> Self {
        Self::Config(value)
    }
}

impl From<PermissionGroupStoreError> for PermissionGroupManagerError {
    fn from(value: PermissionGroupStoreError) -> Self {
        Self::Store(value)
    }
}

/// Fallible edit failure from `PermissionGroupManager::try_update_config`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionGroupUpdateError<E> {
    /// The caller rejected its edit before persistence.
    Edit(E),
    /// The edited config failed validation or persistence.
    Manager(PermissionGroupManagerError),
}

impl<E: fmt::Display> fmt::Display for PermissionGroupUpdateError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Edit(error) => error.fmt(formatter),
            Self::Manager(error) => error.fmt(formatter),
        }
    }
}

impl<E> Error for PermissionGroupUpdateError<E> where E: Error + 'static {}

impl<E> From<PermissionGroupManagerError> for PermissionGroupUpdateError<E> {
    fn from(value: PermissionGroupManagerError) -> Self {
        Self::Manager(value)
    }
}
