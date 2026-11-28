use std::sync::Arc;

use futures::future::BoxFuture;
use steel_utils::{Identifier, locks::SyncMutex};

use super::super::{
    PermissionCollections, PermissionCollectionsConfig, PermissionGroupConfig,
    PermissionGroupManagerError, PermissionGroupStore, PermissionGroupStoreError,
    PermissionGroupUpdateError, PermissionGroupsConfig, PermissionManager,
};

#[derive(Debug)]
struct CapturingStore {
    saved: Arc<SyncMutex<Vec<PermissionGroupsConfig>>>,
}

impl PermissionGroupStore for CapturingStore {
    fn save_groups(
        &self,
        config: PermissionGroupsConfig,
    ) -> BoxFuture<'static, Result<(), PermissionGroupStoreError>> {
        let saved = Arc::clone(&self.saved);
        Box::pin(async move {
            saved.lock().push(config);
            Ok(())
        })
    }

    fn save_collections(
        &self,
        _config: PermissionCollectionsConfig,
    ) -> BoxFuture<'static, Result<(), PermissionGroupStoreError>> {
        Box::pin(async { Ok(()) })
    }

    fn load_groups(
        &self,
    ) -> BoxFuture<'static, Result<PermissionGroupsConfig, PermissionGroupStoreError>> {
        Box::pin(async { Ok(PermissionGroupsConfig::default()) })
    }

    fn load_collections(
        &self,
    ) -> BoxFuture<'static, Result<PermissionCollectionsConfig, PermissionGroupStoreError>> {
        Box::pin(async { Ok(PermissionCollectionsConfig::default()) })
    }
}

#[derive(Debug)]
struct ReloadStore {
    groups: PermissionGroupsConfig,
    collections: PermissionCollectionsConfig,
    saves: Arc<SyncMutex<usize>>,
}

impl PermissionGroupStore for ReloadStore {
    fn save_groups(
        &self,
        _config: PermissionGroupsConfig,
    ) -> BoxFuture<'static, Result<(), PermissionGroupStoreError>> {
        let saves = Arc::clone(&self.saves);
        Box::pin(async move {
            *saves.lock() += 1;
            Ok(())
        })
    }

    fn save_collections(
        &self,
        _config: PermissionCollectionsConfig,
    ) -> BoxFuture<'static, Result<(), PermissionGroupStoreError>> {
        let saves = Arc::clone(&self.saves);
        Box::pin(async move {
            *saves.lock() += 1;
            Ok(())
        })
    }

    fn load_groups(
        &self,
    ) -> BoxFuture<'static, Result<PermissionGroupsConfig, PermissionGroupStoreError>> {
        let groups = self.groups.clone();
        Box::pin(async move { Ok(groups) })
    }

    fn load_collections(
        &self,
    ) -> BoxFuture<'static, Result<PermissionCollectionsConfig, PermissionGroupStoreError>> {
        let collections = self.collections.clone();
        Box::pin(async move { Ok(collections) })
    }
}

#[derive(Debug)]
struct FailingStore;

impl PermissionGroupStore for FailingStore {
    fn save_groups(
        &self,
        _config: PermissionGroupsConfig,
    ) -> BoxFuture<'static, Result<(), PermissionGroupStoreError>> {
        Box::pin(async { Err(PermissionGroupStoreError::new("test store failure")) })
    }

    fn save_collections(
        &self,
        _config: PermissionCollectionsConfig,
    ) -> BoxFuture<'static, Result<(), PermissionGroupStoreError>> {
        Box::pin(async { Err(PermissionGroupStoreError::new("test store failure")) })
    }

    fn load_groups(
        &self,
    ) -> BoxFuture<'static, Result<PermissionGroupsConfig, PermissionGroupStoreError>> {
        Box::pin(async { Err(PermissionGroupStoreError::new("test store failure")) })
    }

    fn load_collections(
        &self,
    ) -> BoxFuture<'static, Result<PermissionCollectionsConfig, PermissionGroupStoreError>> {
        Box::pin(async { Err(PermissionGroupStoreError::new("test store failure")) })
    }
}

fn manager(store: Option<Arc<dyn PermissionGroupStore>>) -> PermissionManager {
    match PermissionManager::new(
        PermissionGroupsConfig::default(),
        PermissionCollectionsConfig::default(),
        store,
    ) {
        Ok(manager) => manager,
        Err(error) => panic!("default permission groups should resolve: {error}"),
    }
}

fn config_with_builder() -> PermissionGroupsConfig {
    let mut config = PermissionGroupsConfig::default();
    config.groups.insert(
        "builder".to_owned(),
        PermissionGroupConfig {
            allow: vec!["steel:build".to_owned()],
            ..PermissionGroupConfig::default()
        },
    );
    config
}

#[tokio::test]
async fn manager_persists_before_publishing_replacement() {
    let saved = Arc::new(SyncMutex::new(Vec::new()));
    let manager = manager(Some(Arc::new(CapturingStore {
        saved: Arc::clone(&saved),
    })));
    let config = config_with_builder();

    let result = manager.replace_config(config.clone()).await;
    assert_eq!(result, Ok(()));
    assert!(manager.contains_group("builder"));
    assert_eq!(&*saved.lock(), &[config]);
}

#[tokio::test]
async fn manager_keeps_state_when_store_fails() {
    let manager = manager(Some(Arc::new(FailingStore)));

    let result = manager.replace_config(config_with_builder()).await;
    assert!(matches!(result, Err(PermissionGroupManagerError::Store(_))));
    assert!(!manager.contains_group("builder"));
    assert!(
        !manager
            .group_config_snapshot()
            .groups
            .contains_key("builder")
    );
}

#[tokio::test]
async fn manager_keeps_state_when_validation_fails() {
    let saved = Arc::new(SyncMutex::new(Vec::new()));
    let manager = manager(Some(Arc::new(CapturingStore {
        saved: Arc::clone(&saved),
    })));
    let mut invalid = PermissionGroupsConfig::default();
    let op_group = invalid.op_group.clone();
    invalid.groups.remove(&op_group);

    let result = manager.replace_config(invalid).await;
    assert!(matches!(
        result,
        Err(PermissionGroupManagerError::Config(_))
    ));
    assert!(manager.contains_group(&op_group));
    assert!(saved.lock().is_empty());
}

#[tokio::test]
async fn manager_updates_the_latest_config_under_one_lock() {
    let saved = Arc::new(SyncMutex::new(Vec::new()));
    let manager = Arc::new(manager(Some(Arc::new(CapturingStore {
        saved: Arc::clone(&saved),
    }))));
    let first = {
        let manager = Arc::clone(&manager);
        tokio::spawn(async move {
            manager
                .update_config(|config| {
                    config
                        .groups
                        .insert("first".to_owned(), PermissionGroupConfig::default());
                })
                .await
        })
    };
    let second = {
        let manager = Arc::clone(&manager);
        tokio::spawn(async move {
            manager
                .update_config(|config| {
                    config
                        .groups
                        .insert("second".to_owned(), PermissionGroupConfig::default());
                })
                .await
        })
    };

    let first = match first.await {
        Ok(result) => result,
        Err(error) => panic!("first update task should complete: {error}"),
    };
    let second = match second.await {
        Ok(result) => result,
        Err(error) => panic!("second update task should complete: {error}"),
    };
    assert_eq!(first, Ok(()));
    assert_eq!(second, Ok(()));
    let config = manager.group_config_snapshot();
    assert!(config.groups.contains_key("first"));
    assert!(config.groups.contains_key("second"));
    assert_eq!(saved.lock().len(), 2);
}

#[tokio::test]
async fn fallible_update_skips_unchanged_and_rejected_writes() {
    let saved = Arc::new(SyncMutex::new(Vec::new()));
    let manager = manager(Some(Arc::new(CapturingStore {
        saved: Arc::clone(&saved),
    })));

    let unchanged = manager
        .try_update_config(|_config| Ok::<_, &'static str>("unchanged"))
        .await;
    assert_eq!(unchanged, Ok("unchanged"));
    assert!(saved.lock().is_empty());

    let rejected = manager
        .try_update_config(|config| {
            config
                .groups
                .insert("builder".to_owned(), PermissionGroupConfig::default());
            Err::<(), _>("rejected")
        })
        .await;
    assert_eq!(rejected, Err(PermissionGroupUpdateError::Edit("rejected")));
    assert!(!manager.contains_group("builder"));
    assert!(saved.lock().is_empty());
}

#[tokio::test]
async fn manager_reloads_configs_from_store_without_writing_them_back() {
    let saves = Arc::new(SyncMutex::new(0));
    let manager = manager(Some(Arc::new(ReloadStore {
        groups: config_with_builder(),
        collections: PermissionCollectionsConfig::default(),
        saves: Arc::clone(&saves),
    })));

    assert!(!manager.contains_group("builder"));
    assert_eq!(manager.reload_from_store().await, Ok(()));
    assert!(manager.contains_group("builder"));
    assert_eq!(*saves.lock(), 0);
}

#[tokio::test]
async fn manager_reload_preserves_code_declared_collections() {
    let mut declared_config = PermissionCollectionsConfig::default();
    declared_config
        .collections
        .entry("test".to_owned())
        .or_default()
        .insert(
            "declared".to_owned(),
            vec!["steel:test.permission".to_owned()],
        );
    let declared = match PermissionCollections::from_config(&declared_config) {
        Ok(collections) => collections,
        Err(error) => panic!("declared collection should parse: {error}"),
    };
    let declared_id = Identifier::new("test", "declared");
    let manager = manager(Some(Arc::new(ReloadStore {
        groups: PermissionGroupsConfig::default(),
        collections: PermissionCollectionsConfig::default(),
        saves: Arc::new(SyncMutex::new(0)),
    })));
    if let Err(error) = manager.set_declared_collections(declared) {
        panic!("declared collections should resolve: {error}");
    }

    assert_eq!(manager.reload_from_store().await, Ok(()));
    assert!(manager.contains_collection(&declared_id));
}

#[tokio::test]
async fn manager_reload_keeps_state_when_store_fails() {
    let manager = manager(Some(Arc::new(FailingStore)));

    let result = manager.reload_from_store().await;
    assert!(matches!(result, Err(PermissionGroupManagerError::Store(_))));
    assert_eq!(
        manager.group_config_snapshot(),
        PermissionGroupsConfig::default()
    );
}
