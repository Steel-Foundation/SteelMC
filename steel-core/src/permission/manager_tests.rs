use std::sync::Arc;

use futures::future::BoxFuture;
use steel_utils::locks::SyncMutex;

use super::{
    PermissionGroupConfig, PermissionGroupManager, PermissionGroupManagerError,
    PermissionGroupStore, PermissionGroupStoreError, PermissionGroupUpdateError,
    PermissionGroupsConfig,
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
}

fn manager(store: Option<Arc<dyn PermissionGroupStore>>) -> PermissionGroupManager {
    match PermissionGroupManager::new(PermissionGroupsConfig::default(), store) {
        Ok(manager) => manager,
        Err(error) => panic!("default permission groups should resolve: {error}"),
    }
}

fn config_with_builder() -> PermissionGroupsConfig {
    let mut config = PermissionGroupsConfig::default();
    config.groups.insert(
        "builder".to_owned(),
        PermissionGroupConfig {
            allow: vec!["steel.build".to_owned()],
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
    assert!(!manager.config_snapshot().groups.contains_key("builder"));
}

#[tokio::test]
async fn manager_keeps_state_when_validation_fails() {
    let saved = Arc::new(SyncMutex::new(Vec::new()));
    let manager = manager(Some(Arc::new(CapturingStore {
        saved: Arc::clone(&saved),
    })));
    let mut invalid = PermissionGroupsConfig::default();
    invalid.groups.remove("op");

    let result = manager.replace_config(invalid).await;
    assert!(matches!(
        result,
        Err(PermissionGroupManagerError::Config(_))
    ));
    assert!(manager.contains_group("op"));
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
    let config = manager.config_snapshot();
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
