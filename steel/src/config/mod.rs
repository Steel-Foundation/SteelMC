//! Server configuration loading.
//!
//! This module handles loading the server configuration from disk.
//! The config is loaded once at startup, split into creation-time values
//! (consumed by the server constructor) and a `RuntimeConfig` (stored on `Server`).

mod groups;
mod logging;
mod server;

pub use groups::FilePermissionGroupStore;
pub(crate) use logging::DEFAULT_MAX_HISTORY;
pub use logging::{LogConfig, LogLevel, LogTimeFormat, RotationTimeFormat};
pub use server::{ServerConfig, ThreadConfig};

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::Deserialize;
use steel_core::{
    config::WorldsConfig,
    permission::{PermissionGroupStore, PermissionGroupsConfig},
};

use self::{groups::load_or_create_groups, server::validate};

#[cfg(feature = "stand-alone")]
const DEFAULT_FAVICON: &[u8] = include_bytes!("../../../package-content/favicon.png");

const DEFAULT_CONFIG: &str = include_str!("../../../package-content/config.toml");
const DEFAULT_WORLDS: &str = include_str!("../../../package-content/worlds.toml");

/// Top-level TOML deserialization target — used once at startup, not stored globally.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SteelConfig {
    /// The full server configuration (`[server]` section)
    pub server: ServerConfig,
    /// Logging configuration (`[log]` section)
    pub log: Option<LogConfig>,
    /// World and domain configuration from `worlds.toml`.
    #[serde(skip, default = "empty_worlds_config")]
    pub worlds: WorldsConfig,
    /// Permission group configuration from `groups.toml`.
    #[serde(skip, default)]
    pub groups: PermissionGroupsConfig,
    /// Path to the loaded `groups.toml`.
    #[serde(skip, default)]
    pub groups_path: Option<PathBuf>,
}

impl SteelConfig {
    /// Builds the store used for persistence-first permission group updates.
    #[must_use]
    pub fn permission_group_store(&self) -> Option<Arc<dyn PermissionGroupStore>> {
        self.groups_path.as_ref().map(|path| {
            Arc::new(FilePermissionGroupStore::new(path.clone())) as Arc<dyn PermissionGroupStore>
        })
    }
}

const fn empty_worlds_config() -> WorldsConfig {
    WorldsConfig {
        save_path: String::new(),
        seed: None,
        default_gamemode: None,
        difficulty: None,
        storage: None,
        player_storage: None,
        domains: BTreeMap::new(),
    }
}

/// Loads the server configuration from the given path, or creates it if it doesn't exist.
pub fn load_or_create(path: &Path) -> Result<SteelConfig, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("failed to get config directory for {}", path.display()))?;
    let mut config = if path.exists() {
        let config_str = fs::read_to_string(path)
            .map_err(|e| format!("failed to read config file {}: {e}", path.display()))?;
        let config: SteelConfig = toml::from_str(config_str.as_str())
            .map_err(|e| format!("failed to parse config: {e}"))?;
        validate(&config.server).map_err(|e| format!("failed to validate config: {e}"))?;
        config
    } else {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "failed to create config directory {}: {e}",
                parent.display()
            )
        })?;
        fs::write(path, DEFAULT_CONFIG)
            .map_err(|e| format!("failed to write config file {}: {e}", path.display()))?;
        let config: SteelConfig = toml::from_str(DEFAULT_CONFIG)
            .map_err(|e| format!("failed to parse default config: {e}"))?;
        config
    };

    let worlds_path = parent.join("worlds.toml");
    config.worlds = load_or_create_worlds(&worlds_path)?;
    let groups_path = parent.join("groups.toml");
    config.groups = load_or_create_groups(&groups_path)?;
    config.groups_path = Some(groups_path);

    // If icon file doesnt exist, write it
    #[cfg(feature = "stand-alone")]
    if config.server.use_favicon && !Path::new(&config.server.favicon).exists() {
        fs::write(Path::new(&config.server.favicon), DEFAULT_FAVICON).map_err(|e| {
            format!(
                "failed to write favicon file {}: {e}",
                config.server.favicon
            )
        })?;
    }

    Ok(config)
}

fn load_or_create_worlds(path: &Path) -> Result<WorldsConfig, String> {
    if path.exists() {
        let worlds_str = fs::read_to_string(path)
            .map_err(|e| format!("failed to read worlds config file {}: {e}", path.display()))?;
        toml::from_str(worlds_str.as_str())
            .map_err(|e| format!("failed to parse worlds config {}: {e}", path.display()))
    } else {
        fs::write(path, DEFAULT_WORLDS)
            .map_err(|e| format!("failed to write worlds config file {}: {e}", path.display()))?;
        toml::from_str(DEFAULT_WORLDS)
            .map_err(|e| format!("failed to parse default worlds config: {e}"))
    }
}

#[cfg(test)]
mod tests;
