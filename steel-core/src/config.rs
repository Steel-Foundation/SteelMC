//! Handles all the server configuration.
//!
//! The `ServerConfig` struct is defined here, but loading is handled by the `steel` crate.
//! Steel-core accesses config via `STEEL_CONFIG` after steel initializes it.

use std::ops::Deref;
use std::sync::OnceLock;

use serde::Deserialize;
use steel_protocol::packet_traits::CompressionInfo;
use steel_protocol::packets::config::{CServerLinks, Link, ServerLinksType};
use steel_utils::codec::Or;
use text_components::TextComponent;

/// Reference to the server configuration.
///
/// This is initialized by the `steel` crate during server startup.
/// Access configuration via the `STEEL_CONFIG` static.
pub struct ServerConfigRef;

static CONFIG: OnceLock<&'static ServerConfig> = OnceLock::new();

impl ServerConfigRef {
    /// Initializes the configuration reference.
    ///
    /// # Panics
    /// Panics if called more than once.
    pub fn init(config: &'static ServerConfig) {
        CONFIG
            .set(config)
            .expect("Server config already initialized");
    }
}

impl Deref for ServerConfigRef {
    type Target = ServerConfig;

    fn deref(&self) -> &Self::Target {
        CONFIG
            .get()
            .expect("Server config not initialized - steel crate must call ServerConfigRef::init()")
    }
}

/// The server configuration.
///
/// Access via `STEEL_CONFIG` static after initialization by the steel crate.
pub static STEEL_CONFIG: ServerConfigRef = ServerConfigRef;

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

/// The different types of world generators that can be used.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordGeneratorTypes {
    /// produces a flat gras world
    Flat,
    /// creates an empty world which can be used for test
    Empty,
}

/// Configuration for world storage.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldStorageConfig {
    /// Standard disk persistence using region files.
    Disk {
        /// Path to the world directory (e.g., "world/overworld").
        path: String,
    },
    /// RAM-only storage with empty chunks created on demand.
    /// No data is persisted - useful for testing and minigames.
    RamOnly,
}

/// The server configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// The Minecraft version this server supports.
    /// This is not loaded from config but set programmatically.
    #[serde(skip)]
    pub mc_version: &'static str,
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
    /// Defines which generator should be used for the world.
    pub world_generator: WordGeneratorTypes,
    /// Defines which storage format and storage option should be used for the world
    pub world_storage_config: WorldStorageConfig,
    /// The compression settings for the server.
    pub compression: Option<CompressionInfo>,
    /// All settings and configurations for server links
    pub server_links: Option<ServerLinks>,
}
