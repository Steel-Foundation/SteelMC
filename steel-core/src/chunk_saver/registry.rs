//! Runtime registry for world storage backends.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;
use serde::Deserialize;
use steel_utils::Identifier;

use crate::config::{StorageSelection, WorldStorageConfig, validate_relative_path};

/// Storage paths and backend for a loaded world.
pub struct WorldStorageOutput {
    /// Chunk storage backend config.
    pub storage: WorldStorageConfig,
    /// Directory containing level data, if persistent.
    pub level_data_path: Option<PathBuf>,
}

struct WorldStorageFactory {
    validate: fn(&toml::Value) -> Result<(), String>,
    create: fn(&toml::Value, &Path, &Path) -> Result<WorldStorageOutput, String>,
}

/// Registry of server-side world storage factories.
pub struct WorldStorageRegistry {
    factories: FxHashMap<Identifier, WorldStorageFactory>,
}

impl WorldStorageRegistry {
    /// Creates a registry containing Steel's built-in world storage backends.
    pub fn new_with_builtins() -> Result<Self, String> {
        let mut registry = Self {
            factories: FxHashMap::default(),
        };
        registry.register(
            Identifier::new("steel", "disk"),
            WorldStorageFactory {
                validate: validate_disk_config,
                create: create_disk_storage,
            },
        )?;
        registry.register(
            Identifier::new("steel", "ram"),
            WorldStorageFactory {
                validate: validate_empty_config,
                create: create_ram_storage,
            },
        )?;
        Ok(registry)
    }

    fn register(&mut self, key: Identifier, factory: WorldStorageFactory) -> Result<(), String> {
        if self.factories.insert(key.clone(), factory).is_some() {
            return Err(format!("duplicate world storage registration {key}"));
        }
        Ok(())
    }

    /// Validates a storage selection.
    pub fn validate_selection(&self, selection: &StorageSelection) -> Result<(), String> {
        let factory = self
            .factories
            .get(&selection.kind)
            .ok_or_else(|| format!("unknown world storage {}", selection.kind))?;
        (factory.validate)(&selection.config_value())
    }

    /// Creates a resolved world storage config.
    pub fn create(
        &self,
        selection: &StorageSelection,
        save_root: &Path,
        default_world_path: &Path,
    ) -> Result<WorldStorageOutput, String> {
        let factory = self
            .factories
            .get(&selection.kind)
            .ok_or_else(|| format!("unknown world storage {}", selection.kind))?;
        (factory.create)(&selection.config_value(), save_root, default_world_path)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiskStorageConfig {
    path: Option<String>,
}

fn validate_disk_config(config: &toml::Value) -> Result<(), String> {
    let parsed: DiskStorageConfig = config
        .clone()
        .try_into()
        .map_err(|e| format!("invalid steel:disk config: {e}"))?;
    if let Some(path) = parsed.path {
        validate_relative_path(&path, "storage.config.path")?;
    }
    Ok(())
}

fn validate_empty_config(config: &toml::Value) -> Result<(), String> {
    let Some(table) = config.as_table() else {
        return Err("storage config must be a table".to_owned());
    };
    if !table.is_empty() {
        return Err("this storage backend does not accept config".to_owned());
    }
    Ok(())
}

fn create_disk_storage(
    config: &toml::Value,
    save_root: &Path,
    default_world_path: &Path,
) -> Result<WorldStorageOutput, String> {
    let parsed: DiskStorageConfig = config
        .clone()
        .try_into()
        .map_err(|e| format!("invalid steel:disk config: {e}"))?;
    let path = parsed.path.map_or_else(
        || default_world_path.to_path_buf(),
        |path| save_root.join(path),
    );
    Ok(WorldStorageOutput {
        storage: WorldStorageConfig::Disk {
            path: path_to_string(path.join("region")),
        },
        level_data_path: Some(path),
    })
}

fn create_ram_storage(
    config: &toml::Value,
    _save_root: &Path,
    _default_world_path: &Path,
) -> Result<WorldStorageOutput, String> {
    validate_empty_config(config)?;
    Ok(WorldStorageOutput {
        storage: WorldStorageConfig::RamOnly,
        level_data_path: None,
    })
}

fn path_to_string(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}
