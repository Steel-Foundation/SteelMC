//! Server configuration loading.
//!
//! This module handles loading the server configuration from disk.
//! The `ServerConfig` struct is defined in `steel-core`, this module
//! just handles the file I/O and initialization.

use base64::{Engine, prelude::BASE64_STANDARD};
use serde::Deserialize;
use std::{fs, path::Path, sync::LazyLock};

// Re-export types from steel-core for convenience
pub use steel_core::config::{ConfigLabel, ConfigLink, ServerConfig, ServerConfigRef, ServerLinks};

#[cfg(feature = "stand-alone")]
const DEFAULT_FAVICON: &[u8] = include_bytes!("../../package-content/favicon.png");
const ICON_PREFIX: &str = "data:image/png;base64,";

const DEFAULT_CONFIG: &str = include_str!("../../package-content/steel_config.json5");

/// The Minecraft version this server supports.
pub const MC_VERSION: &str = "1.21.11";

/// The server configuration.
///
/// This is loaded from `config/steel_config.json5` or created if it doesn't exist.
pub static STEEL_CONFIG: LazyLock<ExtendedConfig> =
    LazyLock::new(|| load_or_create(Path::new("config/steel_config.json5")));

/// A extended configuration for non-server features
#[derive(Debug, Clone, Deserialize)]
pub struct ExtendedConfig {
    /// The full server configuration
    #[serde(flatten)]
    pub server_config: ServerConfig,
    /// Logging configuration
    pub log: Option<LogConfig>,
}

/// Logging configuration
#[derive(Debug, Clone, Deserialize)]
pub struct LogConfig {
    /// Time display format: "none", "date" (HH:MM:SS:mmm), or "uptime" (seconds since start)
    #[serde(default)]
    pub time: LogTimeFormat,
    /// Whether the `module_path` of the log should be displayed
    pub module_path: bool,
    /// Whether the extra data of the log should be displayed
    pub extra: bool,
}

/// Time format for log entries
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
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

/// Loads the server configuration from the given path, or creates it if it doesn't exist.
///
/// # Panics
/// This function will panic if the config file does not exist and the directory cannot be created,
/// or if the config file cannot be read or written.
#[must_use]
fn load_or_create(path: &Path) -> ExtendedConfig {
    let mut config = if path.exists() {
        let config_str = fs::read_to_string(path).expect("Failed to read config file");
        let config: ExtendedConfig =
            serde_json5::from_str(config_str.as_str()).expect("Failed to parse config");
        validate(&config.server_config).expect("Failed to validate config");
        config
    } else {
        fs::create_dir_all(path.parent().expect("Failed to get config directory"))
            .expect("Failed to create config directory");
        fs::write(path, DEFAULT_CONFIG).expect("Failed to write config file");
        let config: ExtendedConfig =
            serde_json5::from_str(DEFAULT_CONFIG).expect("Failed to parse config");
        validate(&config.server_config).expect("Failed to validate config");
        config
    };

    // Set the MC version (not loaded from config file)
    config.server_config.mc_version = MC_VERSION;

    // If icon file doesnt exist, write it
    #[cfg(feature = "stand-alone")]
    if config.server_config.use_favicon && !Path::new(&config.server_config.favicon).exists() {
        fs::write(Path::new(&config.server_config.favicon), DEFAULT_FAVICON)
            .expect("Failed to write favicon file");
    }

    config
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

/// Loads the favicon from the path specified in the config.
/// If the favicon doesn't exist, it will use the default favicon.
/// If `use_favicon` is false, this will return `None`.
///
/// The returned string is a base64 encoded png with the `data:image/png;base64,` prefix.
#[must_use]
pub fn load_favicon(config: &ServerConfig) -> Option<String> {
    if config.use_favicon {
        let path = Path::new(&config.favicon);
        if path.exists() {
            let icon = fs::read(path);

            if let Ok(icon) = icon {
                let cap = ICON_PREFIX.len() + icon.len().div_ceil(3) * 4;
                let mut base64 = String::with_capacity(cap);

                base64 += ICON_PREFIX;
                BASE64_STANDARD.encode_string(icon, &mut base64);

                return Some(base64);
            }
            #[cfg(feature = "stand-alone")]
            {
                let cap = ICON_PREFIX.len() + DEFAULT_FAVICON.len().div_ceil(3) * 4;
                let mut base64 = String::with_capacity(cap);

                base64 += ICON_PREFIX;
                BASE64_STANDARD.encode_string(DEFAULT_FAVICON, &mut base64);

                return Some(base64);
            }
            #[cfg(not(feature = "stand-alone"))]
            return None;
        }
    }
    None
}

/// Initializes the steel-core config reference.
///
/// This must be called before any steel-core code accesses `STEEL_CONFIG`.
pub fn init_steel_core_config() {
    // Force config to load, then initialize steel-core's reference
    ServerConfigRef::init(&STEEL_CONFIG.server_config);
}
