//! Server configuration loading.
//!
//! This module handles loading the server configuration from disk.
//! The config is loaded once at startup, split into creation-time values
//! (consumed by the server constructor) and a `RuntimeConfig` (stored on `Server`).

use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::filter::Directive;

use steel_core::config::{CompressionInfo, RuntimeConfig, ServerLinks, WorldsConfig};

#[cfg(feature = "stand-alone")]
const DEFAULT_FAVICON: &[u8] = include_bytes!("../../package-content/favicon.png");

const DEFAULT_CONFIG: &str = include_str!("../../package-content/config.toml");
const DEFAULT_WORLDS: &str = include_str!("../../package-content/worlds.toml");

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
    /// The view distance of the server.
    pub view_distance: u8,
    /// The simulation distance of the server.
    pub simulation_distance: u8,
    /// Whether the server is in online mode.
    pub online_mode: bool,
    /// Whether the server should use encryption.
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
        }
    }
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

/// Validates the server configuration.
///
/// # Errors
/// This function will return an error if the configuration is invalid.
fn validate(config: &ServerConfig) -> Result<(), &'static str> {
    if !(1..=32).contains(&config.view_distance) {
        return Err("View distance must in range 1..32");
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

    #[test]
    fn packaged_configs_parse() {
        let config: SteelConfig = toml::from_str(DEFAULT_CONFIG).expect("default config parses");
        assert!(!config.server.allow_flight);
        assert_eq!(config.server.chat_spam_threshold_seconds, 10);
        assert_eq!(config.server.command_spam_threshold_seconds, 10);
        validate(&config.server).expect("default config validates");
        let worlds: WorldsConfig = toml::from_str(DEFAULT_WORLDS).expect("default worlds parses");
        assert!(!worlds.domains.is_empty());
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
