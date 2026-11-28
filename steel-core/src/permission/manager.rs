use std::{fmt, sync::Arc};

use futures::future::BoxFuture;
use steel_utils::{
    Identifier,
    locks::{AsyncMutex, SyncRwLock},
};

use super::{
    PermissionCollections, PermissionCollectionsConfig, PermissionConfigError, PermissionGroups,
    PermissionGroupsConfig, PermissionMetadataSet, PermissionSet,
};

/// Persists permission configuration owned outside `steel-core`.
pub trait PermissionGroupStore: Send + Sync {
    /// Saves the complete permission group configuration.
    fn save_groups(
        &self,
        config: PermissionGroupsConfig,
    ) -> BoxFuture<'static, Result<(), PermissionGroupStoreError>>;

    /// Saves the complete permission collection configuration.
    fn save_collections(
        &self,
        config: PermissionCollectionsConfig,
    ) -> BoxFuture<'static, Result<(), PermissionGroupStoreError>>;

    /// Reads the current permission group configuration from storage.
    fn load_groups(
        &self,
    ) -> BoxFuture<'static, Result<PermissionGroupsConfig, PermissionGroupStoreError>>;

    /// Reads the current permission collection configuration from storage.
    fn load_collections(
        &self,
    ) -> BoxFuture<'static, Result<PermissionCollectionsConfig, PermissionGroupStoreError>>;
}

/// Permission persistence failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct PermissionManagerState {
    group_config: PermissionGroupsConfig,
    collections_config: PermissionCollectionsConfig,
    declared_collections: PermissionCollections,
    collections: PermissionCollections,
    groups: PermissionGroups,
}

/// Runtime permission groups and collections with serialized, persistence-first updates.
pub struct PermissionManager {
    updates: AsyncMutex<()>,
    state: SyncRwLock<PermissionManagerState>,
    store: Option<Arc<dyn PermissionGroupStore>>,
}

impl fmt::Debug for PermissionManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PermissionGroupManager")
            .field("updates", &self.updates)
            .field("state", &self.state)
            .field("store", &self.store.as_ref().map(|_| "<group store>"))
            .finish()
    }
}

impl PermissionManager {
    /// Builds a manager from typed config and an optional persistence store.
    ///
    /// # Errors
    ///
    /// Returns an error when the initial config or collections do not resolve.
    pub fn new(
        config: PermissionGroupsConfig,
        collections_config: PermissionCollectionsConfig,
        store: Option<Arc<dyn PermissionGroupStore>>,
    ) -> Result<Self, PermissionConfigError> {
        let declared_collections = PermissionCollections::new();
        let collections = resolve_collections(&collections_config, &declared_collections)?;
        let groups = PermissionGroups::from_config(config.clone(), &collections)?;
        Ok(Self {
            updates: AsyncMutex::new(()),
            state: SyncRwLock::new(PermissionManagerState {
                group_config: config,
                collections_config,
                declared_collections,
                collections,
                groups,
            }),
            store,
        })
    }

    /// Builds a manager without persistence.
    ///
    /// # Errors
    ///
    /// Returns an error when the initial config or collections do not resolve.
    pub fn transient(
        config: PermissionGroupsConfig,
        collections_config: PermissionCollectionsConfig,
    ) -> Result<Self, PermissionConfigError> {
        Self::new(config, collections_config, None)
    }

    /// Returns a typed snapshot of the current groups config.
    #[must_use]
    pub fn group_config_snapshot(&self) -> PermissionGroupsConfig {
        self.state.read().group_config.clone()
    }

    /// Returns a typed snapshot of the current collections config.
    #[must_use]
    pub fn collections_config_snapshot(&self) -> PermissionCollectionsConfig {
        self.state.read().collections_config.clone()
    }

    /// Returns the effective permission collections.
    #[must_use]
    pub fn collections_snapshot(&self) -> PermissionCollections {
        self.state.read().collections.clone()
    }

    /// Returns whether a configured group exists.
    #[must_use]
    pub fn contains_group(&self, group: &str) -> bool {
        self.state.read().groups.contains_group(group)
    }

    /// Returns whether an effective collection exists.
    #[must_use]
    pub fn contains_collection(&self, collection: &Identifier) -> bool {
        self.state.read().collections.contains(collection)
    }

    /// Returns the group assigned by `/op`.
    #[must_use]
    pub fn op_group(&self) -> String {
        self.state.read().group_config.op_group.clone()
    }

    /// Installs the permission collections declared by code at runtime.
    ///
    /// Declared contributions are never persisted. This rebuilds the resolved
    /// groups and collections in place and is meant to run once at startup, after
    /// commands register and extensions declare their collections.
    ///
    /// # Errors
    ///
    /// Returns an error when the rebuilt groups do not resolve.
    pub(crate) fn set_declared_collections(
        &self,
        declared_collections: PermissionCollections,
    ) -> Result<(), PermissionConfigError> {
        let state = self.state.read();
        let config = state.group_config.clone();
        let collections_config = state.collections_config.clone();
        drop(state);

        let collections = resolve_collections(&collections_config, &declared_collections)?;
        let groups = PermissionGroups::from_config(config.clone(), &collections)?;
        *self.state.write() = PermissionManagerState {
            group_config: config,
            collections_config,
            declared_collections,
            collections,
            groups,
        };
        Ok(())
    }

    /// Returns configured group names sorted by name.
    #[must_use]
    pub fn group_names(&self) -> Vec<String> {
        self.state.read().groups.groups().keys().cloned().collect()
    }

    /// Returns effective collection ids sorted by name.
    #[must_use]
    pub fn collection_names(&self) -> Vec<Identifier> {
        self.state
            .read()
            .collections
            .ids()
            .cloned()
            .collect::<Vec<_>>()
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

    /// Replaces the complete groups config after validation and optional persistence.
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

    /// Reloads both configs from storage without writing them back.
    ///
    /// Code-declared collections are preserved and re-resolved with the loaded overrides.
    ///
    /// # Errors
    ///
    /// Returns an error when no store is configured, a read fails, or either config is invalid.
    pub async fn reload_from_store(&self) -> Result<(), PermissionGroupManagerError> {
        let Some(store) = &self.store else {
            return Err(
                PermissionGroupStoreError::new("permission group store is not configured").into(),
            );
        };
        let _guard = self.updates.lock().await;
        let group_config = store.load_groups().await?;
        let collections_config = store.load_collections().await?;
        let declared_collections = self.state.read().declared_collections.clone();
        let collections = resolve_collections(&collections_config, &declared_collections)?;
        let groups = PermissionGroups::from_config(group_config.clone(), &collections)?;
        *self.state.write() = PermissionManagerState {
            group_config,
            collections_config,
            declared_collections,
            collections,
            groups,
        };
        Ok(())
    }

    /// Updates the latest groups config under the manager update lock.
    ///
    /// # Errors
    ///
    /// Returns an error when the updated config is invalid or cannot be persisted.
    pub async fn update_config(
        &self,
        update: impl FnOnce(&mut PermissionGroupsConfig) + Send,
    ) -> Result<(), PermissionGroupManagerError> {
        let _guard = self.updates.lock().await;
        let mut config = self.state.read().group_config.clone();
        update(&mut config);
        self.replace_config_locked(config).await
    }

    /// Updates the latest groups config with a fallible caller-owned edit.
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
        let current = self.state.read().group_config.clone();
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

    /// Updates the latest collections config under the manager update lock.
    ///
    /// # Errors
    ///
    /// Returns an error when the updated config is invalid or cannot be persisted.
    pub async fn update_collections(
        &self,
        update: impl FnOnce(&mut PermissionCollectionsConfig) + Send,
    ) -> Result<(), PermissionGroupManagerError> {
        let _guard = self.updates.lock().await;
        let mut collections_config = self.state.read().collections_config.clone();
        update(&mut collections_config);
        self.replace_collections_locked(collections_config).await
    }

    async fn replace_config_locked(
        &self,
        config: PermissionGroupsConfig,
    ) -> Result<(), PermissionGroupManagerError> {
        let (collections_config, declared_collections) = {
            let state = self.state.read();
            (
                state.collections_config.clone(),
                state.declared_collections.clone(),
            )
        };

        let collections = resolve_collections(&collections_config, &declared_collections)?;
        let groups = PermissionGroups::from_config(config.clone(), &collections)?;
        if let Some(store) = &self.store {
            store.save_groups(config.clone()).await?;
        }
        *self.state.write() = PermissionManagerState {
            group_config: config,
            collections_config,
            declared_collections,
            collections,
            groups,
        };
        Ok(())
    }

    async fn replace_collections_locked(
        &self,
        collections_config: PermissionCollectionsConfig,
    ) -> Result<(), PermissionGroupManagerError> {
        let (config, declared_collections) = {
            let state = self.state.read();
            (
                state.group_config.clone(),
                state.declared_collections.clone(),
            )
        };

        let collections = resolve_collections(&collections_config, &declared_collections)?;
        let groups = PermissionGroups::from_config(config.clone(), &collections)?;
        if let Some(store) = &self.store {
            store.save_collections(collections_config.clone()).await?;
        }
        *self.state.write() = PermissionManagerState {
            group_config: config,
            collections_config,
            declared_collections,
            collections,
            groups,
        };
        Ok(())
    }
}

fn resolve_collections(
    config: &PermissionCollectionsConfig,
    declared_collections: &PermissionCollections,
) -> Result<PermissionCollections, PermissionConfigError> {
    let overrides = PermissionCollections::from_config(config)?;
    Ok(PermissionCollections::effective(
        declared_collections,
        &overrides,
    ))
}

/// Permission group manager update failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PermissionGroupManagerError {
    /// The candidate config is invalid.
    #[error("invalid permission groups config: {0}")]
    Config(#[from] PermissionConfigError),
    /// The candidate config could not be persisted.
    #[error("failed to store permission groups: {0}")]
    Store(#[from] PermissionGroupStoreError),
}

/// Fallible edit failure from `PermissionGroupManager::try_update_config`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PermissionGroupUpdateError<E> {
    /// The caller rejected its edit before persistence.
    #[error("{0}")]
    Edit(E),
    /// The edited config failed validation or persistence.
    #[error(transparent)]
    Manager(#[from] PermissionGroupManagerError),
}
