use std::{
    fs, io,
    path::{Path, PathBuf},
};

use futures::future::BoxFuture;
use steel_core::whitelist::{WhitelistConfig, WhitelistStore, WhitelistStoreError};
use tokio::fs as async_fs;

const DEFAULT_WHITELIST: &str = "enabled = false\nentries = []\n";

/// TOML-backed whitelist store.
#[derive(Clone, Debug)]
pub struct FileWhitelistStore {
    path: PathBuf,
}

impl FileWhitelistStore {
    /// Creates a file-backed whitelist store.
    #[must_use]
    pub const fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl WhitelistStore for FileWhitelistStore {
    fn save(&self, config: WhitelistConfig) -> BoxFuture<'static, Result<(), WhitelistStoreError>> {
        let path = self.path.clone();
        Box::pin(async move {
            let serialized = toml::to_string_pretty(&config).map_err(|error| {
                WhitelistStoreError::new(format!("failed to serialize whitelist: {error}"))
            })?;
            write_atomic_config(&path, serialized)
                .await
                .map_err(|error| {
                    WhitelistStoreError::new(format!(
                        "failed to write whitelist {}: {error}",
                        path.display()
                    ))
                })
        })
    }

    fn load(&self) -> BoxFuture<'static, Result<WhitelistConfig, WhitelistStoreError>> {
        let path = self.path.clone();
        Box::pin(async move {
            let contents = async_fs::read_to_string(&path).await.map_err(|error| {
                WhitelistStoreError::new(format!(
                    "failed to read whitelist {}: {error}",
                    path.display()
                ))
            })?;
            toml::from_str(&contents).map_err(|error| {
                WhitelistStoreError::new(format!(
                    "failed to parse whitelist {}: {error}",
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

pub(super) fn load_or_create_whitelist(path: &Path) -> Result<WhitelistConfig, String> {
    if path.exists() {
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("failed to read whitelist {}: {error}", path.display()))?;
        toml::from_str(&contents)
            .map_err(|error| format!("failed to parse whitelist {}: {error}", path.display()))
    } else {
        fs::write(path, DEFAULT_WHITELIST)
            .map_err(|error| format!("failed to write whitelist {}: {error}", path.display()))?;
        Ok(WhitelistConfig::default())
    }
}
