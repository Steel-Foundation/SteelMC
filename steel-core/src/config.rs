//! Handles all the server configuration.
use base64::{Engine, prelude::BASE64_STANDARD};
use serde::Deserialize;
use std::{fs, path::Path, sync::LazyLock};
use steel_protocol::packet_traits::CompressionInfo;
use steel_protocol::packets::config::{CServerLinks, Link, ServerLinksType};
use steel_utils::codec::Or;
use steel_utils::text::TextComponent;

#[cfg(feature = "stand-alone")]
const DEFAULT_FAVICON: &[u8] = include_bytes!("../../package-content/favicon.png");
const ICON_PREFIX: &str = "data:image/png;base64,";

const DEFAULT_CONFIG: &str = include_str!("../../package-content/steel_config.json5");

/// The server configuration.
///
/// This is loaded from `config/steel_config.json5` or created if it doesn't exist.
pub static STEEL_CONFIG: LazyLock<ServerConfig> =
    LazyLock::new(|| ServerConfig::load_or_create(Path::new("config/steel_config.json5")));

/// Label type for server links - either built-in string or custom `TextComponent`
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum ConfigLabel {
    /// Built-in server link type (e.g., "`bug_report`", "website")
    BuiltIn(ServerLinksType),
    /// Custom text component with formatting
    Custom(TextComponent),
}

/// A single server link configuration entry
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigLink {
    /// The label for this link (built-in type or custom `TextComponent`)
    pub label: ConfigLabel,
    /// The URL for this link
    pub url: String,
}

/// Server links configuration
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ServerLinks {
    /// Enable the server links feature
    pub enable: bool,
    /// List of server links to display
    #[serde(default)]
    pub links: Vec<ConfigLink>,
}

impl ServerLinks {
    /// Creates the server link package from the server config
    #[must_use]
    pub fn from_config() -> Option<CServerLinks> {
        let server_links = STEEL_CONFIG.server_links.as_ref()?;

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

/// The server configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// The port the server will listen on.
    pub server_port: u16,
    /// The seed for the world generator.
    pub seed: String,
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
    /// All settings and configurations for server links
    pub server_links: Option<ServerLinks>,
}

impl ServerConfig {
    /// Loads the server configuration from the given path, or creates it if it doesn't exist.
    ///
    /// # Panics
    /// This function will panic if the config file does not exist and the directory cannot be created,
    /// or if the config file cannot be read or written.
    #[must_use]
    pub fn load_or_create(path: &Path) -> Self {
        let config = if path.exists() {
            let config_str = fs::read_to_string(path).expect("Failed to read config file");
            let config: ServerConfig =
                serde_json5::from_str(config_str.as_str()).expect("Failed to parse config");
            config.validate().expect("Failed to validate config");
            config
        } else {
            fs::create_dir_all(path.parent().expect("Failed to get config directory"))
                .expect("Failed to create config directory");
            fs::write(path, DEFAULT_CONFIG).expect("Failed to write config file");
            let config: ServerConfig =
                serde_json5::from_str(DEFAULT_CONFIG).expect("Failed to parse config");
            config.validate().expect("Failed to validate config");
            config
        };

        // If icon file doesnt exist, write it
        #[cfg(feature = "stand-alone")]
        if config.use_favicon && !Path::new(&config.favicon).exists() {
            fs::write(Path::new(&config.favicon), DEFAULT_FAVICON)
                .expect("Failed to write favicon file");
        }

        config
    }

    /// Validates the server configuration.
    ///
    /// # Errors
    /// This function will return an error if the configuration is invalid.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=64).contains(&self.view_distance) {
            return Err("View distance must in range 1..64");
        }
        if !(1..=32).contains(&self.simulation_distance) {
            return Err("Simulation distance must in range 1..32");
        }
        if let Some(compression) = self.compression {
            if compression.threshold.get() < 256 {
                return Err("Compression threshold must be greater than or equal to 256");
            }
            if !(1..=9).contains(&compression.level) {
                return Err("Compression level must be between 1 and 9");
            }
        }
        if self.enforce_secure_chat {
            if !self.online_mode {
                return Err("online_mode must be true when enforce_secure_chat is enabled");
            }
            if !self.encryption {
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
    pub fn load_favicon(&self) -> Option<String> {
        if self.use_favicon {
            let path = Path::new(&self.favicon);
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
}
