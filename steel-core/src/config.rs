//! Server configuration types used at runtime.
//!
//! The full deserialization struct lives in the `steel` crate. Steel-core only
//! defines `RuntimeConfig` (the subset kept after startup) and the world/domain
//! configuration types that both crates share.

use rustc_hash::FxHashSet;
use serde::{Deserialize, Deserializer, de::Error as DeError};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};
pub use steel_protocol::packet_traits::CompressionInfo;
use steel_protocol::packets::config::{CServerLinks, Link, ServerLinksType};
use steel_utils::Identifier;
use steel_utils::codec::Or;
use steel_utils::types::{Difficulty, GameType};
use text_components::TextComponent;
use toml::map::Map;

use crate::chunk_saver::registry::WorldStorageRegistry;
use crate::worldgen::registry::WorldGeneratorRegistry;

/// Runtime server configuration — the subset of settings needed after startup.
///
/// Stored on `Server` and accessed by game logic at runtime.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// The maximum number of players that can be on the server at once.
    pub max_players: u32,
    /// The view distance of the server.
    pub view_distance: u8,
    /// The simulation distance of the server.
    pub simulation_distance: u8,
    /// Whether the server is in online mode.
    pub online_mode: bool,
    /// Whether the server should use encryption.
    pub encryption: bool,
    /// The message of the day.
    pub motd: String,
    /// Whether to use a favicon.
    pub use_favicon: bool,
    /// The path to the favicon.
    pub favicon: String,
    /// Whether to enforce secure chat.
    pub enforce_secure_chat: bool,
    /// The compression settings for the server.
    pub compression: Option<CompressionInfo>,
    /// All settings and configurations for server links.
    pub server_links: Option<ServerLinks>,
}

impl RuntimeConfig {
    /// Builds the `CServerLinks` packet from config, if server links are enabled.
    #[must_use]
    pub fn server_links_packet(&self) -> Option<CServerLinks> {
        let server_links = self.server_links.as_ref()?;

        if !server_links.enable || server_links.links.is_empty() {
            return None;
        }

        let links: Vec<Link> = server_links
            .links
            .iter()
            .map(|config_link| {
                let label = match &config_link.label {
                    ConfigLabel::BuiltIn(link_type) => Or::Left(*link_type),
                    ConfigLabel::Custom(text_component) => Or::Right(text_component.clone()),
                };
                Link::new(label, config_link.url.clone())
            })
            .collect();

        Some(CServerLinks { links })
    }
}

/// Label type for server links — either built-in string or custom `TextComponent`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
#[expect(
    clippy::large_enum_variant,
    reason = "TextComponent variant is common; boxing would add indirection for every use"
)]
pub enum ConfigLabel {
    /// Built-in server link type (e.g., "`bug_report`", "website")
    BuiltIn(ServerLinksType),
    /// Custom text component with formatting
    Custom(TextComponent),
}

/// A single server link configuration entry.
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigLink {
    /// The label for this link (built-in type or custom `TextComponent`)
    pub label: ConfigLabel,
    /// The URL for this link
    pub url: String,
}

/// Server links configuration.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ServerLinks {
    /// Enable the server links feature
    pub enable: bool,
    /// List of server links to display
    #[serde(default)]
    pub links: Vec<ConfigLink>,
}

/// Configuration for world storage.
#[derive(Debug, Clone)]
pub enum WorldStorageConfig {
    /// Standard disk persistence using region files.
    Disk {
        /// Path to the world directory (e.g., "world/overworld").
        path: String,
    },
    /// RAM-only storage with empty chunks created on demand.
    /// No data is persisted — useful for testing and minigames.
    RamOnly,
}

/// Parsed `worlds.toml` root.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldsConfig {
    /// Root directory for save data.
    #[serde(default = "default_save_path")]
    pub save_path: String,
    /// Root seed default. Empty or omitted means random.
    #[serde(default)]
    pub seed: Option<String>,
    /// Root default game mode for first-visit player data.
    #[serde(default, deserialize_with = "deserialize_optional_game_type")]
    pub default_gamemode: Option<GameType>,
    /// Root default difficulty for new level data.
    #[serde(default, deserialize_with = "deserialize_optional_difficulty")]
    pub difficulty: Option<Difficulty>,
    /// Root world storage default.
    #[serde(default)]
    pub storage: Option<StorageSelection>,
    /// Global player data storage selection.
    #[serde(default)]
    pub player_storage: Option<StorageSelection>,
    /// Domain declarations keyed by domain name.
    pub domains: BTreeMap<String, DomainConfig>,
}

/// Parsed domain config inside `worlds.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainConfig {
    /// Whether this is the server's default domain.
    #[serde(default)]
    pub default: bool,
    /// Domain seed override. Empty means random for this domain.
    #[serde(default)]
    pub seed: Option<String>,
    /// Domain default game mode override.
    #[serde(default, deserialize_with = "deserialize_optional_game_type")]
    pub default_gamemode: Option<GameType>,
    /// Domain difficulty override for new level data.
    #[serde(default, deserialize_with = "deserialize_optional_difficulty")]
    pub difficulty: Option<Difficulty>,
    /// Domain storage override.
    #[serde(default)]
    pub storage: Option<StorageSelection>,
    /// Worlds declared in this domain.
    #[serde(default)]
    pub worlds: Vec<WorldEntryConfig>,
}

/// Parsed world entry inside a domain.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldEntryConfig {
    /// Path part of the loaded world identifier. The domain table key supplies the namespace.
    pub name: String,
    /// Generator factory identifier.
    pub generator: Identifier,
    /// Whether this is the domain's default world.
    #[serde(default)]
    pub default: bool,
    /// World seed override. Empty means random for this world.
    #[serde(default)]
    pub seed: Option<String>,
    /// World default game mode override.
    #[serde(default, deserialize_with = "deserialize_optional_game_type")]
    pub default_gamemode: Option<GameType>,
    /// World difficulty override for new level data.
    #[serde(default, deserialize_with = "deserialize_optional_difficulty")]
    pub difficulty: Option<Difficulty>,
    /// World storage override.
    #[serde(default)]
    pub storage: Option<StorageSelection>,
    /// Generator-specific config. The selected generator validates this strictly.
    #[serde(default)]
    pub config: Option<toml::Value>,
}

/// Registry-backed storage selection from config.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageSelection {
    /// Storage backend identifier.
    #[serde(rename = "type")]
    pub kind: Identifier,
    /// Storage-specific config. The selected storage backend validates this strictly.
    #[serde(default)]
    pub config: Option<toml::Value>,
}

impl StorageSelection {
    /// Default disk world storage.
    #[must_use]
    pub fn default_world_disk() -> Self {
        Self {
            kind: Identifier::new("steel", "disk"),
            config: None,
        }
    }

    /// Default file-backed player storage.
    #[must_use]
    pub fn default_player_file() -> Self {
        Self {
            kind: Identifier::new("steel", "file"),
            config: None,
        }
    }

    /// Returns the config table or an empty table if omitted.
    #[must_use]
    pub fn config_value(&self) -> toml::Value {
        self.config
            .clone()
            .unwrap_or_else(|| toml::Value::Table(Map::new()))
    }
}

/// Fully validated and cascaded worlds config.
#[derive(Debug, Clone)]
pub struct ResolvedWorldsConfig {
    /// Root directory for save data.
    pub save_path: PathBuf,
    /// Default domain name.
    pub default_domain: String,
    /// Resolved domain configs.
    pub domains: Vec<ResolvedDomainConfig>,
    /// Resolved world configs in startup creation order.
    pub worlds: Vec<ResolvedWorldConfig>,
    /// Player data storage selection.
    pub player_storage: StorageSelection,
}

/// Validated domain config.
#[derive(Debug, Clone)]
pub struct ResolvedDomainConfig {
    /// Domain name.
    pub name: String,
    /// Default world identifier in this domain.
    pub default_world: Identifier,
    /// World identifiers in this domain.
    pub worlds: Vec<Identifier>,
}

/// Validated world startup config.
#[derive(Debug, Clone)]
pub struct ResolvedWorldConfig {
    /// Loaded world identifier (`domain:world`).
    pub key: Identifier,
    /// Domain name.
    pub domain: String,
    /// World path/name inside the domain.
    pub name: String,
    /// Generator factory identifier.
    pub generator: Identifier,
    /// Strictly validated generator config.
    pub generator_config: toml::Value,
    /// Resolved world seed.
    pub seed: i64,
    /// Default game mode for first-visit player data in this world.
    pub default_gamemode: GameType,
    /// Difficulty for new level data in this world.
    pub difficulty: Difficulty,
    /// Resolved world storage selection.
    pub storage: StorageSelection,
}

impl WorldsConfig {
    /// Validates `worlds.toml` and resolves cascaded defaults.
    ///
    /// # Errors
    /// Returns a human-readable startup error if any invariant is violated.
    pub fn validate_and_resolve(
        &self,
        generator_registry: &WorldGeneratorRegistry,
        storage_registry: &WorldStorageRegistry,
    ) -> Result<ResolvedWorldsConfig, String> {
        validate_relative_path(&self.save_path, "save_path")?;

        if self.domains.is_empty() {
            return Err("worlds.toml must declare at least one domain".to_owned());
        }

        let root_defaults = RootWorldDefaults {
            seed: seed_from_config(self.seed.as_deref().unwrap_or("")),
            gamemode: self.default_gamemode.unwrap_or(GameType::Survival),
            difficulty: self.difficulty.unwrap_or(Difficulty::Normal),
        };
        let root_storage = self
            .storage
            .clone()
            .unwrap_or_else(StorageSelection::default_world_disk);
        storage_registry.validate_selection(&root_storage)?;

        let player_storage = self
            .player_storage
            .clone()
            .unwrap_or_else(StorageSelection::default_player_file);
        validate_player_storage_selection(&player_storage)?;

        let mut default_domain = None;
        let mut resolved_domains = Vec::with_capacity(self.domains.len());
        let mut resolved_worlds = Vec::new();

        for (domain_name, domain) in &self.domains {
            if domain.default && default_domain.replace(domain_name.clone()).is_some() {
                return Err("worlds.toml must declare exactly one default domain".to_owned());
            }
            let (resolved_domain, mut domain_worlds) = resolve_domain_config(
                domain_name,
                domain,
                root_defaults,
                &root_storage,
                generator_registry,
                storage_registry,
            )?;
            resolved_domains.push(resolved_domain);
            resolved_worlds.append(&mut domain_worlds);
        }

        if resolved_worlds.is_empty() {
            return Err("worlds.toml must declare at least one world".to_owned());
        }

        let Some(default_domain) = default_domain else {
            return Err("worlds.toml must declare exactly one default domain".to_owned());
        };

        Ok(ResolvedWorldsConfig {
            save_path: PathBuf::from(&self.save_path),
            default_domain,
            domains: resolved_domains,
            worlds: resolved_worlds,
            player_storage,
        })
    }
}

fn resolve_domain_config(
    domain_name: &str,
    domain: &DomainConfig,
    root_defaults: RootWorldDefaults,
    root_storage: &StorageSelection,
    generator_registry: &WorldGeneratorRegistry,
    storage_registry: &WorldStorageRegistry,
) -> Result<(ResolvedDomainConfig, Vec<ResolvedWorldConfig>), String> {
    validate_domain_name(domain_name)?;
    if domain.worlds.is_empty() {
        return Err(format!(
            "domain {domain_name} must declare at least one world"
        ));
    }

    let domain_defaults = DomainWorldDefaults::from_config(domain, root_defaults);
    let domain_storage = domain
        .storage
        .clone()
        .unwrap_or_else(|| root_storage.clone());
    storage_registry.validate_selection(&domain_storage)?;

    let mut seen_world_names = FxHashSet::default();
    let mut default_world = None;
    let mut domain_world_ids = Vec::with_capacity(domain.worlds.len());
    let mut resolved_worlds = Vec::with_capacity(domain.worlds.len());

    for world in &domain.worlds {
        let resolved_world = resolve_world_config(
            domain_name,
            world,
            domain_defaults,
            &domain_storage,
            generator_registry,
            storage_registry,
        )?;

        if !seen_world_names.insert(world.name.clone()) {
            return Err(format!(
                "domain {domain_name} declares duplicate world {}",
                world.name
            ));
        }
        if world.default && default_world.replace(resolved_world.key.clone()).is_some() {
            return Err(format!(
                "domain {domain_name} must declare exactly one default world"
            ));
        }

        domain_world_ids.push(resolved_world.key.clone());
        resolved_worlds.push(resolved_world);
    }

    let Some(default_world) = default_world else {
        return Err(format!(
            "domain {domain_name} must declare exactly one default world"
        ));
    };

    Ok((
        ResolvedDomainConfig {
            name: domain_name.to_owned(),
            default_world,
            worlds: domain_world_ids,
        },
        resolved_worlds,
    ))
}

fn resolve_world_config(
    domain_name: &str,
    world: &WorldEntryConfig,
    domain_defaults: DomainWorldDefaults,
    domain_storage: &StorageSelection,
    generator_registry: &WorldGeneratorRegistry,
    storage_registry: &WorldStorageRegistry,
) -> Result<ResolvedWorldConfig, String> {
    validate_world_name(&world.name, domain_name)?;
    let world_key = Identifier::new(domain_name.to_owned(), world.name.clone());

    let generator_config = world
        .config
        .clone()
        .unwrap_or_else(|| toml::Value::Table(Map::new()));
    generator_registry.validate_config(&world.generator, &generator_config)?;

    let storage = world
        .storage
        .clone()
        .unwrap_or_else(|| domain_storage.clone());
    storage_registry.validate_selection(&storage)?;

    Ok(ResolvedWorldConfig {
        key: world_key,
        domain: domain_name.to_owned(),
        name: world.name.clone(),
        generator: world.generator.clone(),
        generator_config,
        seed: world
            .seed
            .as_deref()
            .map_or(domain_defaults.seed, seed_from_config),
        default_gamemode: world.default_gamemode.unwrap_or(domain_defaults.gamemode),
        difficulty: world.difficulty.unwrap_or(domain_defaults.difficulty),
        storage,
    })
}

#[derive(Clone, Copy)]
struct RootWorldDefaults {
    seed: i64,
    gamemode: GameType,
    difficulty: Difficulty,
}

#[derive(Clone, Copy)]
struct DomainWorldDefaults {
    seed: i64,
    gamemode: GameType,
    difficulty: Difficulty,
}

impl DomainWorldDefaults {
    fn from_config(domain: &DomainConfig, root: RootWorldDefaults) -> Self {
        Self {
            seed: domain.seed.as_deref().map_or(root.seed, seed_from_config),
            gamemode: domain.default_gamemode.unwrap_or(root.gamemode),
            difficulty: domain.difficulty.unwrap_or(root.difficulty),
        }
    }
}

fn default_save_path() -> String {
    "saves".to_owned()
}

fn validate_domain_name(name: &str) -> Result<(), String> {
    if name == "global" {
        return Err("domain name global is reserved".to_owned());
    }
    if name.is_empty() || !Identifier::validate_namespace(name) {
        return Err(format!("invalid domain name {name}"));
    }
    Ok(())
}

fn validate_world_name(name: &str, domain: &str) -> Result<(), String> {
    if name.is_empty() || name.contains('/') || !Identifier::validate_path(name) {
        return Err(format!("invalid world name {name} in domain {domain}"));
    }
    Ok(())
}

/// Validates a config path as a relative path under a Steel-owned root.
pub fn validate_relative_path(path: &str, field: &str) -> Result<(), String> {
    let path = Path::new(path);
    if path.as_os_str().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if path.is_absolute() {
        return Err(format!("{field} must be relative"));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(format!("{field} must be a clean relative path"));
            }
        }
    }
    Ok(())
}

fn validate_player_storage_selection(selection: &StorageSelection) -> Result<(), String> {
    if selection.kind != Identifier::new("steel", "file") {
        return Err(format!("unknown player storage {}", selection.kind));
    }
    if selection.config.is_some() {
        return Err("steel:file player storage does not accept config yet".to_owned());
    }
    Ok(())
}

fn seed_from_config(seed: &str) -> i64 {
    if seed.is_empty() {
        rand::random()
    } else {
        seed.parse().unwrap_or_else(|_| {
            let mut hash: i64 = 0;
            for byte in seed.bytes() {
                hash = hash.wrapping_mul(31).wrapping_add(i64::from(byte));
            }
            hash
        })
    }
}

fn deserialize_optional_game_type<'de, D>(deserializer: D) -> Result<Option<GameType>, D::Error>
where
    D: Deserializer<'de>,
{
    let Some(value) = Option::<String>::deserialize(deserializer)? else {
        return Ok(None);
    };
    parse_game_type(&value).map(Some).map_err(DeError::custom)
}

fn parse_game_type(value: &str) -> Result<GameType, String> {
    match value {
        "survival" => Ok(GameType::Survival),
        "creative" => Ok(GameType::Creative),
        "adventure" => Ok(GameType::Adventure),
        "spectator" => Ok(GameType::Spectator),
        _ => Err(format!("invalid gamemode {value}")),
    }
}

fn deserialize_optional_difficulty<'de, D>(deserializer: D) -> Result<Option<Difficulty>, D::Error>
where
    D: Deserializer<'de>,
{
    let Some(value) = Option::<String>::deserialize(deserializer)? else {
        return Ok(None);
    };
    parse_difficulty(&value).map(Some).map_err(DeError::custom)
}

fn parse_difficulty(value: &str) -> Result<Difficulty, String> {
    match value {
        "peaceful" => Ok(Difficulty::Peaceful),
        "easy" => Ok(Difficulty::Easy),
        "normal" => Ok(Difficulty::Normal),
        "hard" => Ok(Difficulty::Hard),
        _ => Err(format!("invalid difficulty {value}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::{REGISTRY, Registry};

    fn registries() -> (WorldGeneratorRegistry, WorldStorageRegistry) {
        let mut registry = Registry::new_vanilla();
        registry.freeze();
        let _ = REGISTRY.init(registry);
        let generators = WorldGeneratorRegistry::new_with_builtins()
            .expect("built-in generator registry should initialize");
        let storage = WorldStorageRegistry::new_with_builtins()
            .expect("built-in storage registry should initialize");
        (generators, storage)
    }

    fn resolve(input: &str) -> Result<ResolvedWorldsConfig, String> {
        let config: WorldsConfig = toml::from_str(input).expect("worlds config should parse");
        let (generators, storage) = registries();
        config.validate_and_resolve(&generators, &storage)
    }

    #[test]
    fn resolves_cascaded_world_defaults() {
        let resolved = resolve(
            r#"
save_path = "saves"
seed = "1"
default_gamemode = "survival"
difficulty = "normal"

[storage]
type = "steel:disk"

[domains.minecraft]
default = true
seed = "2"
default_gamemode = "adventure"
difficulty = "peaceful"

[[domains.minecraft.worlds]]
name = "overworld"
generator = "minecraft:overworld"
default = true

[[domains.minecraft.worlds]]
name = "the_nether"
generator = "minecraft:the_nether"
seed = "3"
default_gamemode = "creative"
difficulty = "hard"
"#,
        )
        .expect("valid worlds config should resolve");

        assert_eq!(resolved.default_domain, "minecraft");
        assert_eq!(resolved.worlds.len(), 2);
        let overworld = resolved
            .worlds
            .iter()
            .find(|world| world.name == "overworld")
            .expect("overworld should exist");
        assert_eq!(overworld.seed, 2);
        assert_eq!(overworld.default_gamemode, GameType::Adventure);
        assert_eq!(overworld.difficulty, Difficulty::Peaceful);

        let nether = resolved
            .worlds
            .iter()
            .find(|world| world.name == "the_nether")
            .expect("nether should exist");
        assert_eq!(nether.seed, 3);
        assert_eq!(nether.default_gamemode, GameType::Creative);
        assert_eq!(nether.difficulty, Difficulty::Hard);
    }

    #[test]
    fn rejects_reserved_global_domain() {
        let error = resolve(
            r#"
[domains.global]
default = true

[[domains.global.worlds]]
name = "overworld"
generator = "minecraft:overworld"
default = true
"#,
        )
        .expect_err("global domain should be rejected");
        assert!(error.contains("reserved"));
    }

    #[test]
    fn rejects_duplicate_world_names() {
        let error = resolve(
            r#"
[domains.minecraft]
default = true

[[domains.minecraft.worlds]]
name = "overworld"
generator = "minecraft:overworld"
default = true

[[domains.minecraft.worlds]]
name = "overworld"
generator = "minecraft:overworld"
"#,
        )
        .expect_err("duplicate world names should be rejected");
        assert!(error.contains("duplicate world"));
    }

    #[test]
    fn rejects_missing_default_world() {
        let error = resolve(
            r#"
[domains.minecraft]
default = true

[[domains.minecraft.worlds]]
name = "overworld"
generator = "minecraft:overworld"
"#,
        )
        .expect_err("missing default world should be rejected");
        assert!(error.contains("default world"));
    }

    #[test]
    fn rejects_unknown_generator() {
        let error = resolve(
            r#"
[domains.minecraft]
default = true

[[domains.minecraft.worlds]]
name = "overworld"
generator = "example:unknown"
default = true
"#,
        )
        .expect_err("unknown generator should be rejected");
        assert!(error.contains("unknown world generator"));
    }

    #[test]
    fn flat_generator_config_is_optional() {
        let resolved = resolve(
            r#"
[domains.minecraft]
default = true

[[domains.minecraft.worlds]]
name = "overworld"
generator = "minecraft:flat"
default = true
"#,
        )
        .expect("flat generator should default its config");

        assert_eq!(
            resolved.worlds[0].generator,
            Identifier::vanilla_static("flat")
        );
    }
}
