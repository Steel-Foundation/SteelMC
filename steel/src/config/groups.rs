use std::{
    fs, io,
    path::{Path, PathBuf},
};

use futures::future::BoxFuture;
use serde::Serialize;
use steel_core::permission::{
    PermissionCollections, PermissionCollectionsConfig, PermissionGroupConfig,
    PermissionGroupStore, PermissionGroupStoreError, PermissionGroups, PermissionGroupsConfig,
    PermissionMetadataRuleConfig, PermissionMetadataValue,
};
use tokio::fs as async_fs;
use toml::ser::Error as TomlSerializeError;

pub(super) const DEFAULT_GROUPS: &str = include_str!("../../../package-content/groups.toml");
pub(super) const DEFAULT_COLLECTIONS: &str =
    include_str!("../../../package-content/collections.toml");
pub(super) const GROUPS_CONFIG_HEADER: &str = concat!(
    "#:schema https://raw.githubusercontent.com/Steel-Foundation/SteelMC/refs/heads/master/",
    "package-content/groups.schema.json\n",
    "# Documentation: https://steelmc.dev/configuration/permissions/\n\n",
);
pub(super) const COLLECTIONS_CONFIG_HEADER: &str = concat!(
    "#:schema https://raw.githubusercontent.com/Steel-Foundation/SteelMC/refs/heads/master/",
    "package-content/collections.schema.json\n",
    "# Documentation: https://steelmc.dev/configuration/permissions/\n\n",
);

/// TOML-backed permission group and collection store.
#[derive(Clone, Debug)]
pub struct FilePermissionGroupStore {
    groups_path: PathBuf,
    collections_path: PathBuf,
}

impl FilePermissionGroupStore {
    /// Creates a file-backed permission store.
    #[must_use]
    pub const fn new(groups_path: PathBuf, collections_path: PathBuf) -> Self {
        Self {
            groups_path,
            collections_path,
        }
    }
}

impl PermissionGroupStore for FilePermissionGroupStore {
    fn save_groups(
        &self,
        config: PermissionGroupsConfig,
    ) -> BoxFuture<'static, Result<(), PermissionGroupStoreError>> {
        let path = self.groups_path.clone();
        Box::pin(async move {
            let serialized = serialize_groups_config(&config).map_err(|error| {
                PermissionGroupStoreError::new(format!(
                    "failed to serialize groups config: {error}"
                ))
            })?;
            write_atomic_config(&path, serialized)
                .await
                .map_err(|error| {
                    PermissionGroupStoreError::new(format!(
                        "failed to write groups config {}: {error}",
                        path.display()
                    ))
                })
        })
    }

    fn save_collections(
        &self,
        config: PermissionCollectionsConfig,
    ) -> BoxFuture<'static, Result<(), PermissionGroupStoreError>> {
        let path = self.collections_path.clone();
        Box::pin(async move {
            let serialized = serialize_collections_config(&config).map_err(|error| {
                PermissionGroupStoreError::new(format!(
                    "failed to serialize collections config: {error}"
                ))
            })?;
            write_atomic_config(&path, serialized)
                .await
                .map_err(|error| {
                    PermissionGroupStoreError::new(format!(
                        "failed to write collections config {}: {error}",
                        path.display()
                    ))
                })
        })
    }

    fn load_groups(
        &self,
    ) -> BoxFuture<'static, Result<PermissionGroupsConfig, PermissionGroupStoreError>> {
        let path = self.groups_path.clone();
        Box::pin(async move {
            let contents = async_fs::read_to_string(&path).await.map_err(|error| {
                PermissionGroupStoreError::new(format!(
                    "failed to read groups config {}: {error}",
                    path.display()
                ))
            })?;
            toml::from_str(&contents).map_err(|error| {
                PermissionGroupStoreError::new(format!(
                    "failed to parse groups config {}: {error}",
                    path.display()
                ))
            })
        })
    }

    fn load_collections(
        &self,
    ) -> BoxFuture<'static, Result<PermissionCollectionsConfig, PermissionGroupStoreError>> {
        let path = self.collections_path.clone();
        Box::pin(async move {
            let contents = async_fs::read_to_string(&path).await.map_err(|error| {
                PermissionGroupStoreError::new(format!(
                    "failed to read collections config {}: {error}",
                    path.display()
                ))
            })?;
            toml::from_str(&contents).map_err(|error| {
                PermissionGroupStoreError::new(format!(
                    "failed to parse collections config {}: {error}",
                    path.display()
                ))
            })
        })
    }
}

fn serialize_groups_config(config: &PermissionGroupsConfig) -> Result<String, TomlSerializeError> {
    let mut output = String::from(GROUPS_CONFIG_HEADER);
    push_toml_field(&mut output, "default_groups", &config.default_groups)?;
    push_toml_field(&mut output, "op_group", &config.op_group)?;
    push_toml_field(
        &mut output,
        "command_block_groups",
        &config.command_block_groups,
    )?;

    for (name, group) in &config.groups {
        output.push('\n');
        output.push_str("[groups.");
        output.push_str(name);
        output.push_str("]\n");
        push_group_config(&mut output, group)?;
    }

    Ok(output)
}

fn serialize_collections_config(
    config: &PermissionCollectionsConfig,
) -> Result<String, TomlSerializeError> {
    let mut output = String::from(COLLECTIONS_CONFIG_HEADER);
    for (namespace, collections) in &config.collections {
        output.push('\n');
        output.push_str("[collections.");
        output.push_str(&toml_key(namespace)?);
        output.push_str("]\n");
        for (name, members) in collections {
            let key = toml_key(name)?;
            push_string_array_field(&mut output, &key, members)?;
        }
    }

    Ok(output)
}

fn push_group_config(
    output: &mut String,
    group: &PermissionGroupConfig,
) -> Result<(), TomlSerializeError> {
    push_toml_field(output, "priority", &group.priority)?;
    push_string_array_field(output, "inherits", &group.inherits)?;
    push_string_array_field(output, "allow", &group.allow)?;
    push_string_array_field(output, "deny", &group.deny)?;
    push_metadata_rules(output, &group.metadata)
}

fn push_string_array_field(
    output: &mut String,
    key: &str,
    values: &[String],
) -> Result<(), TomlSerializeError> {
    if values.is_empty() {
        output.push_str(key);
        output.push_str(" = []\n");
        return Ok(());
    }

    output.push_str(key);
    output.push_str(" = [\n");
    for value in values {
        output.push_str("    ");
        output.push_str(&toml_value(value)?);
        output.push_str(",\n");
    }
    output.push_str("]\n");
    Ok(())
}

fn push_metadata_rules(
    output: &mut String,
    metadata: &[PermissionMetadataRuleConfig],
) -> Result<(), TomlSerializeError> {
    if metadata.is_empty() {
        output.push_str("metadata = []\n");
        return Ok(());
    }

    output.push_str("metadata = [\n");
    for entry in metadata {
        output.push_str("    { key = ");
        output.push_str(&toml_value(&entry.key)?);
        output.push_str(", value = ");
        output.push_str(&permission_metadata_value_toml(&entry.value)?);
        output.push_str(" },\n");
    }
    output.push_str("]\n");
    Ok(())
}

fn push_toml_field<T: Serialize + ?Sized>(
    output: &mut String,
    key: &str,
    value: &T,
) -> Result<(), TomlSerializeError> {
    output.push_str(key);
    output.push_str(" = ");
    output.push_str(&toml_value(value)?);
    output.push('\n');
    Ok(())
}

fn toml_value<T: Serialize + ?Sized>(value: &T) -> Result<String, TomlSerializeError> {
    #[derive(Serialize)]
    struct Field<'a, T: ?Sized> {
        value: &'a T,
    }

    let serialized = toml::to_string(&Field { value })?;
    let serialized = serialized.trim_end();
    Ok(serialized
        .strip_prefix("value = ")
        .unwrap_or(serialized)
        .to_owned())
}

fn toml_key(key: &str) -> Result<String, TomlSerializeError> {
    toml_value(key)
}

fn permission_metadata_value_toml(
    value: &PermissionMetadataValue,
) -> Result<String, TomlSerializeError> {
    match value {
        PermissionMetadataValue::Bool(value) => toml_value(value),
        PermissionMetadataValue::Integer(value) => toml_value(value),
        PermissionMetadataValue::String(value) => toml_value(value),
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

pub(super) fn load_or_create_groups(
    path: &Path,
    collections: &PermissionCollections,
) -> Result<PermissionGroupsConfig, String> {
    let config: PermissionGroupsConfig = if path.exists() {
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("failed to read groups config {}: {error}", path.display()))?;
        toml::from_str(&contents)
            .map_err(|error| format!("failed to parse groups config {}: {error}", path.display()))?
    } else {
        create_parent_dir(path)?;
        fs::write(path, DEFAULT_GROUPS).map_err(|error| {
            format!("failed to write groups config {}: {error}", path.display())
        })?;
        toml::from_str(DEFAULT_GROUPS)
            .map_err(|error| format!("failed to parse default groups config: {error}"))?
    };
    PermissionGroups::from_config(config.clone(), collections).map_err(|error| {
        format!(
            "failed to validate groups config {}: {error}",
            path.display()
        )
    })?;
    Ok(config)
}

pub(super) fn load_or_create_collections(
    path: &Path,
) -> Result<PermissionCollectionsConfig, String> {
    if path.exists() {
        let contents = fs::read_to_string(path).map_err(|error| {
            format!(
                "failed to read collections config {}: {error}",
                path.display()
            )
        })?;
        toml::from_str(&contents).map_err(|error| {
            format!(
                "failed to parse collections config {}: {error}",
                path.display()
            )
        })
    } else {
        create_parent_dir(path)?;
        fs::write(path, DEFAULT_COLLECTIONS).map_err(|error| {
            format!(
                "failed to write collections config {}: {error}",
                path.display()
            )
        })?;
        toml::from_str(DEFAULT_COLLECTIONS)
            .map_err(|error| format!("failed to parse default collections config: {error}"))
    }
}

fn create_parent_dir(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "failed to create config directory {}: {error}",
            parent.display()
        )
    })
}
