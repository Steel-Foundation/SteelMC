//! Mod manifest structure compatible with fabric.mod.json / steel.mod.json format.

use std::collections::HashMap;

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

/// Target environment for a mod.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModEnvironment {
    /// Works on both client and server.
    #[default]
    Both,
    /// Client-only mod.
    Client,
    /// Server-only mod.
    Server,
}

/// Metadata and definition of a Steel mod (compatible with `fabric.mod.json` or `steel.mod.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModManifest {
    /// Schema format version (typically 1 for fabric.mod.json).
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,

    /// Unique mod identifier (alphanumeric, lowercase, underscores/hyphens allowed).
    pub id: String,

    /// Version of the mod.
    pub version: Version,

    /// Display name of the mod.
    #[serde(default)]
    pub name: Option<String>,

    /// Description of the mod.
    #[serde(default)]
    pub description: Option<String>,

    /// Target environment (server/client/both).
    #[serde(default)]
    pub environment: ModEnvironment,

    /// Map of entrypoints (e.g. "main" -> list of native entrypoint symbol names or shared library paths).
    #[serde(default)]
    pub entrypoints: HashMap<String, Vec<String>>,

    /// Mod dependencies (`mod_id` -> Version Requirement).
    #[serde(default)]
    pub depends: HashMap<String, VersionReq>,

    /// Optional mod dependencies.
    #[serde(default)]
    pub recommends: HashMap<String, VersionReq>,

    /// Incompatible mods (`mod_id` -> Version Requirement).
    #[serde(default)]
    pub breaks: HashMap<String, VersionReq>,

    /// Authors or contributors.
    #[serde(default)]
    pub authors: Vec<String>,

    /// License identifier (e.g. AGPL-3.0-or-later, MIT).
    #[serde(default)]
    pub license: Option<String>,
}

fn default_schema_version() -> u32 {
    1
}

impl ModManifest {
    /// Parses a manifest JSON string.
    ///
    /// # Errors
    /// Returns a `serde_json::Error` if parsing or validation fails.
    pub fn parse_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}
