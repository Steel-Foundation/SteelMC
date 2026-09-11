use std::{
    fs, io,
    path::{Path, PathBuf},
};

use futures::future::BoxFuture;
use steel_core::ban::{
    BanListConfig, BanListStore, BanListStoreError, IpBanListConfig, IpBanListStore,
};
use tokio::fs as async_fs;

const DEFAULT_BAN_LIST: &str = "bans = []\n";

/// TOML-backed ban list store.
#[derive(Clone, Debug)]
pub struct FileBanListStore {
    path: PathBuf,
}

impl FileBanListStore {
    /// Creates a file-backed ban list store.
    #[must_use]
    pub const fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl BanListStore for FileBanListStore {
    fn save_bans(
        &self,
        config: BanListConfig,
    ) -> BoxFuture<'static, Result<(), BanListStoreError>> {
        let path = self.path.clone();
        Box::pin(async move {
            let serialized = toml::to_string_pretty(&config).map_err(|error| {
                BanListStoreError::new(format!("failed to serialize ban list: {error}"))
            })?;
            write_atomic_config(&path, serialized)
                .await
                .map_err(|error| {
                    BanListStoreError::new(format!(
                        "failed to write ban list {}: {error}",
                        path.display()
                    ))
                })
        })
    }
}

async fn write_atomic_config(path: &Path, contents: String) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "config path has no parent",
        ));
    };
    async_fs::create_dir_all(parent).await?;
    let temp_path = path.with_extension("toml.tmp");
    async_fs::write(&temp_path, contents).await?;
    async_fs::rename(temp_path, path).await
}

pub(super) fn load_or_create_ban_list(path: &Path) -> Result<BanListConfig, String> {
    if path.exists() {
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("failed to read ban list {}: {error}", path.display()))?;
        toml::from_str(&contents)
            .map_err(|error| format!("failed to parse ban list {}: {error}", path.display()))
    } else {
        fs::write(path, DEFAULT_BAN_LIST)
            .map_err(|error| format!("failed to write ban list {}: {error}", path.display()))?;
        Ok(BanListConfig::default())
    }
}

/// TOML-backed IP ban list store.
#[derive(Clone, Debug)]
pub struct FileIpBanListStore {
    path: PathBuf,
}

impl FileIpBanListStore {
    /// Creates a file-backed IP ban list store.
    #[must_use]
    pub const fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl IpBanListStore for FileIpBanListStore {
    fn save_bans(
        &self,
        config: IpBanListConfig,
    ) -> BoxFuture<'static, Result<(), BanListStoreError>> {
        let path = self.path.clone();
        Box::pin(async move {
            let serialized = toml::to_string_pretty(&config).map_err(|error| {
                BanListStoreError::new(format!("failed to serialize IP ban list: {error}"))
            })?;
            write_atomic_config(&path, serialized)
                .await
                .map_err(|error| {
                    BanListStoreError::new(format!(
                        "failed to write IP ban list {}: {error}",
                        path.display()
                    ))
                })
        })
    }
}

pub(super) fn load_or_create_ip_ban_list(path: &Path) -> Result<IpBanListConfig, String> {
    if path.exists() {
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("failed to read IP ban list {}: {error}", path.display()))?;
        toml::from_str(&contents)
            .map_err(|error| format!("failed to parse IP ban list {}: {error}", path.display()))
    } else {
        fs::write(path, DEFAULT_BAN_LIST)
            .map_err(|error| format!("failed to write IP ban list {}: {error}", path.display()))?;
        Ok(IpBanListConfig::default())
    }
}
