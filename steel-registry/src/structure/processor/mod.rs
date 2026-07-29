//! Structure processor-list registry data.
//!
//! Processor lists are registry data in vanilla and in datapacks. Steel stores
//! them typed but leaves registry references as identifiers so future modded
//! registries can resolve or replace them without reshaping configured features.

use std::sync::OnceLock;

use rustc_hash::FxHashMap;
use steel_utils::Identifier;

pub mod data;

pub use data::*;

/// A registered structure processor list.
#[derive(Debug)]
pub struct StructureProcessorList {
    /// Registry key.
    pub key: Identifier,
    /// Typed processor-list payload.
    pub data: StructureProcessorListData,
    /// Cached registry ID.
    pub id: OnceLock<usize>,
}

/// Read-only structure processor-list reference.
pub type StructureProcessorListRef = &'static StructureProcessorList;

/// Registry of structure processor lists.
pub struct StructureProcessorListRegistry {
    lists_by_id: Vec<StructureProcessorListRef>,
    lists_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl StructureProcessorListRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            lists_by_id: Vec::new(),
            lists_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers a processor list and returns its numeric ID.
    pub fn register(&mut self, entry: StructureProcessorListRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register StructureProcessorList after registry has been frozen"
        );
        let id = self.lists_by_id.len();
        let cached = entry.id.get_or_init(|| id);
        assert_eq!(
            *cached, id,
            "structure processor list registered with conflicting id"
        );
        self.lists_by_id.push(entry);
        self.lists_by_key.insert(entry.key.clone(), id);
        id
    }

    /// Iterates over all processor lists.
    pub fn iter(&self) -> impl Iterator<Item = (usize, StructureProcessorListRef)> + '_ {
        self.lists_by_id
            .iter()
            .enumerate()
            .map(|(id, &entry)| (id, entry))
    }
}

impl Default for StructureProcessorListRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry_ext!(
    StructureProcessorListRegistry,
    StructureProcessorList,
    lists_by_id,
    lists_by_key
);

crate::impl_registry_entry_eq!(StructureProcessorList);

impl crate::RegistryEntry for StructureProcessorList {
    fn key(&self) -> &Identifier {
        &self.key
    }

    fn try_id(&self) -> Option<usize> {
        self.id.get().copied()
    }
}
