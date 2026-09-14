//! Typed brain memory module types

use std::fmt::{self, Debug, Formatter};
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::OnceLock;

use rustc_hash::FxHashMap;
use steel_utils::{DowncastType, DowncastTypeKey, Identifier};

use super::MemoryValue;

/// A registered memory module type, erased of its value type.
pub type MemoryModuleTypeRef = &'static MemoryModuleTypeEntry;

/// The registry identity of one brain memory, without its value type.
#[derive(Debug, PartialEq, Eq)]
pub struct MemoryModuleTypeEntry {
    /// Registry key identifying this memory.
    pub key: Identifier,
    value_type_key: DowncastTypeKey,
}

impl MemoryModuleTypeEntry {
    /// Returns the downcast key of the Rust value this memory stores.
    #[must_use]
    pub const fn value_type_key(&self) -> DowncastTypeKey {
        self.value_type_key
    }
}

/// Typed handle for one brain memory.
pub struct MemoryModuleType<V: MemoryValue + DowncastType> {
    entry: MemoryModuleTypeEntry,
    _marker: PhantomData<fn(&V)>,
}

impl<V: MemoryValue + DowncastType> MemoryModuleType<V> {
    /// Creates a memory module type that stores `V` under `key`.
    #[must_use]
    pub const fn new(key: Identifier) -> Self {
        Self {
            entry: MemoryModuleTypeEntry {
                key,
                value_type_key: V::TYPE_KEY,
            },
            _marker: PhantomData,
        }
    }

    /// Returns the erased registry entry for this memory.
    #[must_use]
    pub const fn entry(&'static self) -> MemoryModuleTypeRef {
        &self.entry
    }

    /// Returns the registry key.
    #[must_use]
    pub const fn key(&self) -> &Identifier {
        &self.entry.key
    }
}

impl<V: MemoryValue + DowncastType> Debug for MemoryModuleType<V> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryModuleType")
            .field("key", &self.entry.key)
            .field("value_type_key", &self.entry.value_type_key)
            .finish()
    }
}

/// Registry of brain memory module types.
pub struct MemoryModuleTypeRegistry {
    memory_types: Vec<MemoryModuleTypeRef>,
    by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl MemoryModuleTypeRegistry {
    /// Creates an empty, still-writable registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            memory_types: Vec::new(),
            by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers a memory module type and returns its registry id.
    ///
    /// # Panics
    ///
    /// Panics if the registry is frozen or the key is already registered.
    pub fn register<V: MemoryValue + DowncastType>(
        &mut self,
        memory_type: &'static MemoryModuleType<V>,
    ) -> usize {
        self.register_entry(memory_type.entry())
    }

    fn register_entry(&mut self, entry: MemoryModuleTypeRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register memory module types after the registry has been frozen"
        );
        assert!(
            !self.by_key.contains_key(&entry.key),
            "Cannot register duplicate memory module type key: {}",
            entry.key
        );
        let id = self.memory_types.len();
        self.memory_types.push(entry);
        self.by_key.insert(entry.key.clone(), id);
        id
    }

    /// Rejects further registrations.
    pub const fn freeze(&mut self) {
        self.allows_registering = false;
    }

    /// Returns the memory module type registered under `key`.
    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<MemoryModuleTypeRef> {
        self.by_key
            .get(key)
            .and_then(|&id| self.memory_types.get(id))
            .copied()
    }

    /// Returns the memory module type registered under `id`.
    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<MemoryModuleTypeRef> {
        self.memory_types.get(id).copied()
    }

    /// Iterates registered memory module types in registration order.
    pub fn iter(&self) -> impl Iterator<Item = MemoryModuleTypeRef> + '_ {
        self.memory_types.iter().copied()
    }

    /// Returns the number of registered memory module types.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.memory_types.len()
    }

    /// Returns whether no memory module type is registered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.memory_types.is_empty()
    }
}

impl Default for MemoryModuleTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper for the global memory module type registry that implements `Deref`.
pub struct MemoryModuleTypeRegistryLock(OnceLock<MemoryModuleTypeRegistry>);

impl MemoryModuleTypeRegistryLock {
    fn get_or_init(
        &self,
        init: impl FnOnce() -> MemoryModuleTypeRegistry,
    ) -> &MemoryModuleTypeRegistry {
        self.0.get_or_init(init)
    }
}

impl Deref for MemoryModuleTypeRegistryLock {
    type Target = MemoryModuleTypeRegistry;

    fn deref(&self) -> &Self::Target {
        self.0
            .get()
            .expect("Memory module type registry not initialized")
    }
}

/// Global memory module type registry.
///
/// Access via deref: `MEMORY_MODULE_TYPES.by_key(&key)`.
pub static MEMORY_MODULE_TYPES: MemoryModuleTypeRegistryLock =
    MemoryModuleTypeRegistryLock(OnceLock::new());

/// Initializes the global memory module type registry, once the main registry is
/// frozen.
pub(in crate::entity::ai::brain) fn init_memory_module_types() {
    MEMORY_MODULE_TYPES.get_or_init(|| {
        let mut registry = MemoryModuleTypeRegistry::new();
        registry.freeze();
        registry
    });
}
