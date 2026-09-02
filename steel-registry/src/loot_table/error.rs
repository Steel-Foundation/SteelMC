use steel_utils::Identifier;
use thiserror::Error;

/// A world lookup required before loot evaluation can finish.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExplorationMapRequest {
    pub destination: Identifier,
    pub decoration: Identifier,
    pub zoom: i32,
    pub search_radius: i32,
    pub skip_existing_chunks: bool,
}

/// A loot-table evaluation failure that must not publish partial results.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LootError {
    #[error("unknown {registry} registry value {key}")]
    UnknownRegistryValue {
        registry: &'static str,
        key: Identifier,
    },
    #[error("unknown {registry} registry tag {key}")]
    UnknownRegistryTag {
        registry: &'static str,
        key: Identifier,
    },
    #[error("loot evaluation requires an origin for {0}")]
    MissingOrigin(&'static str),
    #[error("loot evaluation requires level access for {0}")]
    MissingLevel(&'static str),
    #[error("loot evaluation requires exploration-map world data")]
    ExplorationMapRequired(ExplorationMapRequest),
    #[error("exploration-map zoom {0} is outside Vanilla's signed-byte range")]
    InvalidExplorationMapZoom(i32),
    #[error("one loot evaluation produced too many exploration maps")]
    TooManyExplorationMaps,
    #[error("unsupported loot number provider: {0}")]
    UnsupportedNumberProvider(&'static str),
    #[error("unsupported loot condition: {0}")]
    UnsupportedCondition(&'static str),
    #[error("unsupported loot function: {0}")]
    UnsupportedFunction(&'static str),
    #[error("unsupported loot entry: {0}")]
    UnsupportedEntry(&'static str),
}

pub type LootResult<T> = Result<T, LootError>;
