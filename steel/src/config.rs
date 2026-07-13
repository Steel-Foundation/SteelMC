//! Server configuration loading.
//!
//! This module handles loading the server configuration from disk.
//! The config is loaded once at startup, split into creation-time values
//! (consumed by the server constructor) and a `RuntimeConfig` (stored on `Server`).

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::filter::Directive;

use futures::future::BoxFuture;
use reqwest::Url;
use steel_core::config::{
    CompressionInfo, RuntimeConfig, ServerLinks, WorldsConfig, validate_login_security,
};
use steel_core::permission::{
    PermissionGroupConfig, PermissionGroupStore, PermissionGroupStoreError, PermissionGroups,
    PermissionGroupsConfig, PermissionMetadataRuleConfig, PermissionMetadataValue,
};
use tokio::fs as async_fs;
use toml::ser::Error as TomlSerializeError;

#[cfg(feature = "stand-alone")]
const DEFAULT_FAVICON: &[u8] = include_bytes!("../../package-content/favicon.png");

const DEFAULT_CONFIG: &str = include_str!("../../package-content/config.toml");
const DEFAULT_WORLDS: &str = include_str!("../../package-content/worlds.toml");
const DEFAULT_GROUPS: &str = include_str!("../../package-content/groups.toml");
const GROUPS_CONFIG_HEADER: &str = concat!(
    "#:schema https://raw.githubusercontent.com/Steel-Foundation/SteelMC/refs/heads/master/",
    "package-content/groups.schema.json\n",
    "# Documentation: https://steelmc.dev/configuration/permissions/\n\n",
);

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

/// TOML-backed permission group store.
#[derive(Clone, Debug)]
pub struct FilePermissionGroupStore {
    path: PathBuf,
}

impl FilePermissionGroupStore {
    /// Creates a file-backed group store.
    #[must_use]
    pub const fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl PermissionGroupStore for FilePermissionGroupStore {
    fn save_groups(
        &self,
        config: PermissionGroupsConfig,
    ) -> BoxFuture<'static, Result<(), PermissionGroupStoreError>> {
        let path = self.path.clone();
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
}

fn serialize_groups_config(config: &PermissionGroupsConfig) -> Result<String, TomlSerializeError> {
    let mut output = String::from(GROUPS_CONFIG_HEADER);
    push_toml_field(&mut output, "default_groups", &config.default_groups)?;

    for (name, group) in &config.groups {
        output.push('\n');
        output.push_str("[groups.");
        output.push_str(name);
        output.push_str("]\n");
        push_group_config(&mut output, group)?;
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
    struct Field<'a, T: Serialize + ?Sized> {
        value: &'a T,
    }

    let serialized = toml::to_string(&Field { value })?;
    let serialized = serialized.trim_end();
    Ok(serialized
        .strip_prefix("value = ")
        .unwrap_or(serialized)
        .to_owned())
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

const fn default_spam_threshold_seconds() -> i32 {
    10
}

fn default_log_path() -> String {
    "./.logs".to_string()
}

const fn default_log_file() -> bool {
    true
}

const fn default_max_history() -> usize {
    50
}

/// The full server configuration as deserialized from TOML.
///
/// Contains both creation-time values (seed, world generator, storage)
/// and runtime values that get moved into `RuntimeConfig`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// The port the server will listen on.
    pub server_port: u16,
    /// The maximum number of players that can be on the server at once.
    pub max_players: u32,
    /// Allow `view_distance` above vanilla's 32-chunk cap.
    #[serde(default)]
    pub allow_extended_view_distance: bool,
    /// The view distance of the server.
    pub view_distance: u8,
    /// The simulation distance of the server.
    pub simulation_distance: u8,
    /// Whether the server is in online mode.
    pub online_mode: bool,
    /// Optional authentication endpoint for online-mode `hasJoined` checks.
    pub auth_server: Option<String>,
    /// Optional endpoint for online-mode player name-to-profile lookups.
    pub profile_server: Option<String>,
    /// Whether the server should use encryption. Required in online mode.
    pub encryption: bool,
    /// Whether vanilla floating/flying movement checks permit unauthorized flight.
    #[serde(default)]
    pub allow_flight: bool,
    /// The message of the day.
    pub motd: String,
    /// Whether to use a favicon.
    pub use_favicon: bool,
    /// The path to the favicon.
    pub favicon: String,
    /// Whether to enforce secure chat.
    pub enforce_secure_chat: bool,
    /// Vanilla chat spam threshold window in seconds
    #[serde(default = "default_spam_threshold_seconds")]
    pub chat_spam_threshold_seconds: i32,
    /// Vanilla command spam threshold window in seconds
    #[serde(default = "default_spam_threshold_seconds")]
    pub command_spam_threshold_seconds: i32,
    /// The compression settings for the server.
    pub compression: Option<CompressionInfo>,
    /// All settings and configurations for server links.
    pub server_links: Option<ServerLinks>,
    /// Thread counts for server thread pools.
    #[serde(default)]
    pub threads: ThreadConfig,
}

impl ServerConfig {
    /// Extracts the `RuntimeConfig` from this full config.
    #[must_use]
    pub fn into_runtime_config(self) -> RuntimeConfig {
        RuntimeConfig {
            max_players: self.max_players,
            view_distance: self.view_distance,
            simulation_distance: self.simulation_distance,
            online_mode: self.online_mode,
            auth_server: self.auth_server,
            profile_server: self.profile_server,
            encryption: self.encryption,
            allow_flight: self.allow_flight,
            motd: self.motd,
            use_favicon: self.use_favicon,
            favicon: self.favicon,
            enforce_secure_chat: self.enforce_secure_chat,
            chat_spam_threshold_seconds: self.chat_spam_threshold_seconds,
            command_spam_threshold_seconds: self.command_spam_threshold_seconds,
            compression: self.compression,
            server_links: self.server_links,
            chunk_generation_threads: self.threads.chunk_generation,
        }
    }
}

/// Optional worker counts for server thread pools.
///
/// A value of `0` or an omitted field uses the pool's automatic default.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThreadConfig {
    /// Worker threads for the primary Tokio runtime.
    pub main_runtime: Option<usize>,
    /// Worker threads for the chunk Tokio runtime.
    pub chunk_runtime: Option<usize>,
    /// Worker threads for the Rayon chunk generation pool.
    pub chunk_generation: Option<usize>,
}

/// Logging configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogConfig {
    /// Path where store the log files and history
    #[serde(default = "default_log_path")]
    pub log_path: String,
    /// The level of information the logger will show
    #[serde(default)]
    pub log_level: LogLevel,
    /// Time display format: "none", "date" (HH:MM:SS:mmm), or "uptime" (seconds since start)
    #[serde(default)]
    pub time: LogTimeFormat,
    /// Whether the `module_path` of the log should be displayed
    #[serde(default)]
    pub module_path: bool,
    /// Whether the extra data of the log should be displayed
    #[serde(default)]
    pub extra: bool,
    /// Whether the log should be written into a file
    #[serde(default = "default_log_file")]
    pub log_file: bool,
    /// Time between log file rotations
    #[serde(default)]
    pub rotation_time: RotationTimeFormat,
    /// Amount of console commands saved
    #[serde(default = "default_max_history")]
    pub max_history: usize,
}

/// Time format for log entries
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogTimeFormat {
    /// No time displayed
    None,
    /// Current time (HH:MM:SS:mmm)
    #[default]
    Date,
    /// Seconds since server start
    Uptime,
}

/// Time for log files rotation
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RotationTimeFormat {
    /// No rotation
    None,
    /// Rotate hourly
    Hourly,
    /// Rotate daily
    #[default]
    Daily,
    /// Rotate weekly
    Weekly,
    /// Rotate monthly
    Monthly,
}

/// The level of information the logger will show
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Only error logs
    Error,
    /// Error and warn logs
    Warn,
    /// All standard logs
    #[default]
    Info,
    /// Standard + Debug info enabled
    Debug,
    /// All logs are shown
    Trace,
}
impl LogLevel {
    /// Converts the log level in it's respective logging directive
    #[must_use]
    pub fn to_directive(self) -> Directive {
        match self {
            LogLevel::Error => LevelFilter::ERROR.into(),
            LogLevel::Warn => LevelFilter::WARN.into(),
            LogLevel::Info => LevelFilter::INFO.into(),
            LogLevel::Debug => LevelFilter::DEBUG.into(),
            LogLevel::Trace => LevelFilter::TRACE.into(),
        }
    }
}

/// Loads the server configuration from the given path, or creates it if it doesn't exist.
///
pub fn load_or_create(path: &Path) -> Result<SteelConfig, String> {
    let mut config = if path.exists() {
        let config_str = fs::read_to_string(path)
            .map_err(|e| format!("failed to read config file {}: {e}", path.display()))?;
        let config: SteelConfig = toml::from_str(config_str.as_str())
            .map_err(|e| format!("failed to parse config: {e}"))?;
        validate(&config.server).map_err(|e| format!("failed to validate config: {e}"))?;
        config
    } else {
        let parent = path
            .parent()
            .ok_or_else(|| format!("failed to get config directory for {}", path.display()))?;
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
        validate(&config.server).map_err(|e| format!("failed to validate default config: {e}"))?;
        config
    };

    let worlds_path = path
        .parent()
        .ok_or_else(|| format!("failed to get config directory for {}", path.display()))?
        .join("worlds.toml");
    config.worlds = load_or_create_worlds(&worlds_path)?;
    let groups_path = path
        .parent()
        .ok_or_else(|| format!("failed to get config directory for {}", path.display()))?
        .join("groups.toml");
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

fn load_or_create_groups(path: &Path) -> Result<PermissionGroupsConfig, String> {
    let config: PermissionGroupsConfig = if path.exists() {
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("failed to read groups config {}: {error}", path.display()))?;
        toml::from_str(&contents)
            .map_err(|error| format!("failed to parse groups config {}: {error}", path.display()))?
    } else {
        fs::write(path, DEFAULT_GROUPS).map_err(|error| {
            format!("failed to write groups config {}: {error}", path.display())
        })?;
        toml::from_str(DEFAULT_GROUPS)
            .map_err(|error| format!("failed to parse default groups config: {error}"))?
    };
    PermissionGroups::from_config(config.clone()).map_err(|error| {
        format!(
            "failed to validate groups config {}: {error}",
            path.display()
        )
    })?;
    Ok(config)
}

/// Validates the server configuration.
///
/// # Errors
/// This function will return an error if the configuration is invalid.
fn validate(config: &ServerConfig) -> Result<(), &'static str> {
    validate_login_security(config.online_mode, config.encryption)?;
    if !config.allow_extended_view_distance && !(1..=32).contains(&config.view_distance) {
        return Err("View distance must in range 1..32");
    }
    if config.allow_extended_view_distance && !(1..=127).contains(&config.view_distance) {
        return Err("View distance must in range 1..127");
    }
    if let Some(auth_server) = &config.auth_server {
        let Ok(url) = Url::parse(auth_server) else {
            return Err("auth_server must be an absolute URL");
        };
        if !matches!(url.scheme(), "http" | "https") {
            return Err("auth_server must use http or https");
        }
    }
    if let Some(profile_server) = &config.profile_server {
        let Ok(url) = Url::parse(profile_server) else {
            return Err("profile_server must be an absolute URL");
        };
        if !matches!(url.scheme(), "http" | "https") {
            return Err("profile_server must use http or https");
        }
    }
    if config.simulation_distance > config.view_distance {
        return Err("Simulation distance must be less than or equal to view distance");
    }
    if let Some(compression) = config.compression {
        if compression.threshold.get() < 256 {
            return Err("Compression threshold must be greater than or equal to 256");
        }
        if !(1..=9).contains(&compression.level) {
            return Err("Compression level must be between 1 and 9");
        }
    }
    if config.enforce_secure_chat {
        if !config.online_mode {
            return Err("online_mode must be true when enforce_secure_chat is enabled");
        }
        if !config.encryption {
            return Err("encryption must be true when enforce_secure_chat is enabled");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_config_root(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        env::temp_dir().join(format!("steelmc-config-{name}-{suffix}"))
    }

    #[test]
    fn packaged_configs_parse() {
        let config: SteelConfig = toml::from_str(DEFAULT_CONFIG).expect("default config parses");
        assert!(DEFAULT_CONFIG.starts_with(concat!(
            "#:schema https://raw.githubusercontent.com/Steel-Foundation/SteelMC/refs/heads/master/",
            "package-content/config.schema.json\n",
            "# Documentation: https://steelmc.dev/configuration/server-configuration/\n\n",
        )));
        assert!(!config.server.allow_flight);
        assert_eq!(config.server.chat_spam_threshold_seconds, 10);
        assert_eq!(config.server.command_spam_threshold_seconds, 10);
        validate(&config.server).expect("default config validates");
        let worlds: WorldsConfig = toml::from_str(DEFAULT_WORLDS).expect("default worlds parses");
        assert!(DEFAULT_WORLDS.starts_with(concat!(
            "#:schema https://raw.githubusercontent.com/Steel-Foundation/SteelMC/refs/heads/master/",
            "package-content/worlds.schema.json\n",
            "# Documentation: https://steelmc.dev/configuration/world-configuration/\n\n",
        )));
        assert!(!worlds.domains.is_empty());
        let groups: PermissionGroupsConfig =
            toml::from_str(DEFAULT_GROUPS).expect("default groups parse");
        PermissionGroups::from_config(groups).expect("default groups validate");
        assert!(DEFAULT_GROUPS.starts_with(GROUPS_CONFIG_HEADER));
    }

    #[tokio::test]
    async fn file_permission_group_store_round_trips_typed_config() {
        let root = temp_config_root("groups-store");
        let path = root.join("groups.toml");
        let store = FilePermissionGroupStore::new(path.clone());
        let mut config = PermissionGroupsConfig::default();
        config.groups.insert(
            "builder".to_owned(),
            PermissionGroupConfig {
                allow: vec!["steel.build{domain=survival}".to_owned()],
                metadata: vec![
                    PermissionMetadataRuleConfig {
                        key: "plugin:max_homes{domain=survival}".to_owned(),
                        value: PermissionMetadataValue::Integer(5),
                    },
                    PermissionMetadataRuleConfig {
                        key: "plugin:prefix".to_owned(),
                        value: PermissionMetadataValue::String("[Builder]".to_owned()),
                    },
                    PermissionMetadataRuleConfig {
                        key: "plugin:can_claim".to_owned(),
                        value: PermissionMetadataValue::Bool(true),
                    },
                ],
                ..PermissionGroupConfig::default()
            },
        );

        PermissionGroupStore::save_groups(&store, config.clone())
            .await
            .expect("groups config should save");
        let written = async_fs::read_to_string(&path)
            .await
            .expect("groups config should be written");
        let parsed: PermissionGroupsConfig =
            toml::from_str(&written).expect("written config should parse");

        assert_eq!(
            written.lines().next(),
            Some(concat!(
                "#:schema https://raw.githubusercontent.com/Steel-Foundation/SteelMC/refs/heads/master/",
                "package-content/groups.schema.json"
            ))
        );
        assert!(
            written.contains("# Documentation: https://steelmc.dev/configuration/permissions/")
        );
        assert!(written.contains("{ key = \"plugin:max_homes{domain=survival}\", value = 5 }"));
        assert!(written.contains("{ key = \"plugin:prefix\", value = \"[Builder]\" }"));
        assert!(written.contains("{ key = \"plugin:can_claim\", value = true }"));
        assert_eq!(parsed, config);
        PermissionGroups::from_config(parsed).expect("written groups should validate");
        async_fs::remove_dir_all(root)
            .await
            .expect("temporary config directory should be removable");
    }

    #[test]
    fn server_config_defaults_allow_flight_to_false() {
        let input = r#"
            [server]
            server_port = 25565
            max_players = 20
            view_distance = 10
            simulation_distance = 10
            online_mode = true
            encryption = true
            motd = "A Steel Server"
            use_favicon = false
            favicon = "config/favicon.png"
            enforce_secure_chat = false
            chat_spam_threshold_seconds = 10
            command_spam_threshold_seconds = 10
        "#;

        let config: SteelConfig = toml::from_str(input).expect("config should parse");

        assert!(!config.server.allow_flight);
    }

    #[test]
    fn configured_auth_server_flows_to_runtime_config() {
        let auth_server = "https://auth.example.com/session/minecraft/hasJoined";
        let config_toml = DEFAULT_CONFIG.replace(
            "online_mode = true",
            &format!("online_mode = true\nauth_server = \"{auth_server}\""),
        );
        let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

        assert_eq!(config.server.auth_server.as_deref(), Some(auth_server));
        assert_eq!(
            config.server.into_runtime_config().auth_server.as_deref(),
            Some(auth_server)
        );
    }

    #[test]
    fn configured_profile_server_flows_to_runtime_config() {
        let profile_server = "https://profiles.example.com/lookup/name";
        let config_toml = DEFAULT_CONFIG.replace(
            "online_mode = true",
            &format!("online_mode = true\nprofile_server = \"{profile_server}\""),
        );
        let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

        assert_eq!(
            config.server.profile_server.as_deref(),
            Some(profile_server)
        );
        assert_eq!(
            config
                .server
                .into_runtime_config()
                .profile_server
                .as_deref(),
            Some(profile_server)
        );
    }

    #[test]
    fn configured_thread_counts_parse_and_generation_flows_to_runtime_config() {
        let config_toml = DEFAULT_CONFIG
            .replace("main_runtime = 0", "main_runtime = 3")
            .replace("chunk_runtime = 0", "chunk_runtime = 4")
            .replace("chunk_generation = 0", "chunk_generation = 5");
        let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

        assert_eq!(config.server.threads.main_runtime, Some(3));
        assert_eq!(config.server.threads.chunk_runtime, Some(4));
        assert_eq!(config.server.threads.chunk_generation, Some(5));
        assert_eq!(
            config.server.into_runtime_config().chunk_generation_threads,
            Some(5)
        );
    }

    #[test]
    fn validate_rejects_extended_view_distance_without_opt_in() {
        let config_toml = DEFAULT_CONFIG.replace("view_distance = 10", "view_distance = 33");
        let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

        assert_eq!(
            validate(&config.server),
            Err("View distance must in range 1..32")
        );
    }

    #[test]
    fn validate_rejects_online_mode_without_encryption() {
        let config_toml = DEFAULT_CONFIG.replace("encryption = true", "encryption = false");
        let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

        assert_eq!(
            validate(&config.server),
            Err("encryption must be true when online_mode is enabled")
        );
    }

    #[test]
    fn validate_allows_offline_mode_without_encryption() {
        let config_toml = DEFAULT_CONFIG
            .replace("online_mode = true", "online_mode = false")
            .replace("encryption = true", "encryption = false");
        let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

        validate(&config.server).expect("offline mode does not require encryption");
    }

    #[test]
    fn validate_allows_extended_view_distance_with_opt_in() {
        let config_toml = DEFAULT_CONFIG
            .replace(
                "allow_extended_view_distance = false",
                "allow_extended_view_distance = true",
            )
            .replace("view_distance = 10", "view_distance = 127")
            .replace("simulation_distance = 10", "simulation_distance = 127");
        let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

        validate(&config.server).expect("extended view distance validates");
    }

    #[test]
    fn validate_rejects_invalid_auth_server_url() {
        let config_toml = DEFAULT_CONFIG.replace(
            "online_mode = true",
            "online_mode = true\nauth_server = \"not a url\"",
        );
        let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

        assert_eq!(
            validate(&config.server),
            Err("auth_server must be an absolute URL")
        );
    }

    #[test]
    fn validate_allows_http_auth_server_url() {
        let config_toml = DEFAULT_CONFIG.replace(
            "online_mode = true",
            "online_mode = true\nauth_server = \"http://localhost:8080/session/minecraft/hasJoined\"",
        );
        let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

        validate(&config.server).expect("http auth server URL validates");
    }

    #[test]
    fn validate_rejects_unsupported_auth_server_scheme() {
        let config_toml = DEFAULT_CONFIG.replace(
            "online_mode = true",
            "online_mode = true\nauth_server = \"ftp://auth.example.com/session/minecraft/hasJoined\"",
        );
        let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

        assert_eq!(
            validate(&config.server),
            Err("auth_server must use http or https")
        );
    }

    #[test]
    fn server_config_defaults_spam_thresholds_for_older_configs() {
        let input = r#"
            [server]
            server_port = 25565
            max_players = 20
            view_distance = 10
            simulation_distance = 10
            online_mode = true
            encryption = true
            motd = "A Steel Server"
            use_favicon = false
            favicon = "config/favicon.png"
            enforce_secure_chat = false
        "#;

        let config: SteelConfig = toml::from_str(input).expect("config should parse");

        assert_eq!(config.server.chat_spam_threshold_seconds, 10);
        assert_eq!(config.server.command_spam_threshold_seconds, 10);
    }

    #[test]
    fn log_config_defaults_for_older_configs() {
        let input = r#"
            [server]
            server_port = 25565
            max_players = 20
            view_distance = 10
            simulation_distance = 10
            online_mode = true
            encryption = true
            motd = "A Steel Server"
            use_favicon = false
            favicon = "config/favicon.png"
            enforce_secure_chat = false

            [log]
            time = "uptime"
            module_path = false
            extra = false
        "#;

        let config: SteelConfig = toml::from_str(input).expect("older log config should parse");
        let log_config = config.log.expect("log config should be present");

        assert_eq!(log_config.log_path, "./.logs");
        assert_eq!(log_config.log_level, LogLevel::Info);
        assert!(log_config.log_file);
        assert_eq!(log_config.rotation_time, RotationTimeFormat::Daily);
        assert_eq!(log_config.max_history, 50);
    }
}
