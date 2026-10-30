//! Configured and placed feature registries.
//!
//! Configured features describe *what* to place. Placed features pair a
//! configured feature with vanilla's ordered placement modifier chain.

use std::sync::OnceLock;

use rustc_hash::FxHashMap;
use steel_utils::Identifier;

pub mod data;

pub use data::*;

/// A fully-configured feature registry entry.
#[derive(Debug)]
pub struct ConfiguredFeature {
    /// Registry key.
    pub key: Identifier,
    /// Typed feature configuration.
    pub kind: ConfiguredFeatureKind,
    /// Cached registry ID.
    pub id: OnceLock<usize>,
}

/// Read-only configured feature reference.
pub type ConfiguredFeatureEntryRef = &'static ConfiguredFeature;

/// Registry of configured features.
pub struct ConfiguredFeatureRegistry {
    features_by_id: Vec<ConfiguredFeatureEntryRef>,
    features_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl ConfiguredFeatureRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            features_by_id: Vec::new(),
            features_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers a configured feature and returns its numeric ID.
    pub fn register(&mut self, entry: ConfiguredFeatureEntryRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register ConfiguredFeature after registry has been frozen"
        );
        let id = self.features_by_id.len();
        let cached = entry.id.get_or_init(|| id);
        assert_eq!(
            *cached, id,
            "configured feature registered with conflicting id"
        );
        self.features_by_id.push(entry);
        self.features_by_key.insert(entry.key.clone(), id);
        id
    }

    /// Iterates over all configured features.
    pub fn iter(&self) -> impl Iterator<Item = (usize, ConfiguredFeatureEntryRef)> + '_ {
        self.features_by_id
            .iter()
            .enumerate()
            .map(|(id, &entry)| (id, entry))
    }
}

impl Default for ConfiguredFeatureRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry_ext!(
    ConfiguredFeatureRegistry,
    ConfiguredFeature,
    features_by_id,
    features_by_key
);

crate::impl_registry_entry_eq!(ConfiguredFeature);

impl crate::RegistryEntry for ConfiguredFeature {
    fn key(&self) -> &Identifier {
        &self.key
    }

    fn try_id(&self) -> Option<usize> {
        self.id.get().copied()
    }
}

/// A placed feature registry entry.
#[derive(Debug)]
pub struct PlacedFeature {
    /// Registry key.
    pub key: Identifier,
    /// Configured feature plus placement modifier chain.
    pub data: PlacedFeatureData,
    /// Cached registry ID.
    pub id: OnceLock<usize>,
}

/// Read-only placed feature reference.
pub type PlacedFeatureEntryRef = &'static PlacedFeature;

/// Registry of placed features.
pub struct PlacedFeatureRegistry {
    features_by_id: Vec<PlacedFeatureEntryRef>,
    features_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl PlacedFeatureRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            features_by_id: Vec::new(),
            features_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers a placed feature and returns its numeric ID.
    pub fn register(&mut self, entry: PlacedFeatureEntryRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register PlacedFeature after registry has been frozen"
        );
        let id = self.features_by_id.len();
        let cached = entry.id.get_or_init(|| id);
        assert_eq!(*cached, id, "placed feature registered with conflicting id");
        self.features_by_id.push(entry);
        self.features_by_key.insert(entry.key.clone(), id);
        id
    }

    /// Iterates over all placed features.
    pub fn iter(&self) -> impl Iterator<Item = (usize, PlacedFeatureEntryRef)> + '_ {
        self.features_by_id
            .iter()
            .enumerate()
            .map(|(id, &entry)| (id, entry))
    }
}

impl Default for PlacedFeatureRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry_ext!(
    PlacedFeatureRegistry,
    PlacedFeature,
    features_by_id,
    features_by_key
);

crate::impl_registry_entry_eq!(PlacedFeature);

impl crate::RegistryEntry for PlacedFeature {
    fn key(&self) -> &Identifier {
        &self.key
    }

    fn try_id(&self) -> Option<usize> {
        self.id.get().copied()
    }
}

impl simdnbt::ToNbtTag for &BlockStateProvider {
    fn to_nbt_tag(self) -> simdnbt::owned::NbtTag {
        simdnbt::owned::NbtTag::Compound((self.nbt)())
    }
}

/// Registry of block-state providers.
pub struct BlockStateProviderRegistry {
    providers_by_id: Vec<BlockStateProviderRef>,
    providers_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl BlockStateProviderRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            providers_by_id: Vec::new(),
            providers_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers a block-state provider and returns its numeric ID.
    pub fn register(&mut self, entry: BlockStateProviderRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register BlockStateProvider after registry has been frozen"
        );
        let id = self.providers_by_id.len();
        let cached = entry.id.get_or_init(|| id);
        assert_eq!(
            *cached, id,
            "block state provider registered with conflicting id"
        );
        self.providers_by_id.push(entry);
        self.providers_by_key.insert(entry.key.clone(), id);
        id
    }

    /// Iterates over all block-state providers.
    pub fn iter(&self) -> impl Iterator<Item = (usize, BlockStateProviderRef)> + '_ {
        self.providers_by_id
            .iter()
            .enumerate()
            .map(|(id, &entry)| (id, entry))
    }
}

impl Default for BlockStateProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry_ext!(
    BlockStateProviderRegistry,
    BlockStateProvider,
    providers_by_id,
    providers_by_key
);

crate::impl_registry_entry_eq!(BlockStateProvider);

impl crate::RegistryEntry for BlockStateProvider {
    fn key(&self) -> &Identifier {
        &self.key
    }

    fn try_id(&self) -> Option<usize> {
        self.id.get().copied()
    }
}
