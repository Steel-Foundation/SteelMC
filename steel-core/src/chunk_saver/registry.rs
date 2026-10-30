//! Runtime registry for world storage backends.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;
use serde::Deserialize;
use steel_utils::Identifier;

use crate::config::{
    ResolvedWorldsConfig, StorageSelection, WorldStorageConfig, validate_relative_path,
};

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
            Identifier::from_steel("disk"),
            WorldStorageFactory {
                validate: validate_disk_config,
                create: create_disk_storage,
            },
        )?;
        registry.register(
            Identifier::from_steel("ram"),
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

    /// Resolves storage and checks domain clock durability before any world is loaded.
    pub(crate) fn resolve_worlds(
        &self,
        config: &ResolvedWorldsConfig,
    ) -> Result<FxHashMap<Identifier, WorldStorageOutput>, String> {
        let mut outputs = FxHashMap::default();
        for world in &config.worlds {
            let path = config
                .save_path
                .join(&world.domain)
                .join("worlds")
                .join(&world.name);
            let output = self
                .create(&world.storage, &config.save_path, &path)
                .map_err(|error| format!("failed to create storage for {}: {error}", world.key))?;
            outputs.insert(world.key.clone(), output);
        }
        for domain in &config.domains {
            let primary = outputs
                .get(&domain.default_world)
                .ok_or_else(|| format!("domain {} has no primary storage", domain.name))?;
            if primary.level_data_path.is_some() {
                continue;
            }
            for key in &domain.worlds {
                let output = outputs
                    .get(key)
                    .ok_or_else(|| format!("world {key} has no resolved storage"))?;
                if output.level_data_path.is_some()
                    || matches!(output.storage, WorldStorageConfig::Disk { .. })
                {
                    return Err(format!(
                        "domain {} default world {} must persist level data because world {key} uses persistent storage",
                        domain.name, domain.default_world
                    ));
                }
            }
        }
        Ok(outputs)
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

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use crate::config::{DomainConfig, StorageSelection, WorldEntryConfig, WorldsConfig};
    use crate::worldgen::generator::registry::WorldGeneratorRegistry;
    use steel_utils::Identifier;

    use super::WorldStorageRegistry;

    #[test]
    fn domain_storage_accepts_durable_primary_or_entirely_ephemeral_worlds() {
        init_vanilla_registry();
        let storage = WorldStorageRegistry::new_with_builtins().expect("storage registry");
        let generators = WorldGeneratorRegistry::new_with_builtins().expect("generators");
        for (primary, derived) in [("disk", "disk"), ("disk", "ram"), ("ram", "ram")] {
            let worlds = [("primary", primary, true), ("derived", derived, false)]
                .into_iter()
                .map(|(name, backend, default)| WorldEntryConfig {
                    name: name.to_owned(),
                    generator: Identifier::new_static("minecraft", "flat"),
                    default,
                    seed: None,
                    default_gamemode: None,
                    difficulty: None,
                    storage: Some(StorageSelection {
                        kind: Identifier::new_static("steel", backend),
                        config: None,
                    }),
                    nether_portal_target: None,
                    end_portal_target: None,
                    config: None,
                })
                .collect();
            let config = WorldsConfig {
                save_path: "saves".to_owned(),
                seed: None,
                default_gamemode: None,
                difficulty: None,
                storage: None,
                player_storage: None,
                domains: [(
                    "example".to_owned(),
                    DomainConfig {
                        default: true,
                        seed: None,
                        default_gamemode: None,
                        difficulty: None,
                        storage: None,
                        worlds,
                    },
                )]
                .into(),
            };

            let resolved = config
                .validate_and_resolve(&generators, &storage)
                .expect("resolve");
            storage
                .resolve_worlds(&resolved)
                .expect("valid storage combination");
        }
    }
}
