//! Entity data serializer registry.
//!
//! This registry maps serializer names to IDs and stores writer functions,
//! following the same pattern as `DataComponentRegistry`.

use rustc_hash::FxHashMap;
use std::io;

use super::EntityData;

/// Writer function for serializing entity data to network format.
///
/// Takes a reference to the [`EntityData`] value and writes it to the buffer.
/// Returns an error if the value doesn't match the expected serializer type.
pub type EntityDataWriter = fn(&EntityData, &mut Vec<u8>) -> io::Result<()>;

/// Entry for a registered entity data serializer.
pub struct EntityDataSerializerEntry {
    /// The serializer's name (e.g., "byte", "int", "float").
    pub name: &'static str,
    /// The writer function for this serializer.
    pub writer: EntityDataWriter,
}

/// Registry of entity data serializers.
///
/// The serializer ID is determined by registration order, which must match
/// vanilla's `EntityDataSerializers.java` exactly.
pub struct EntityDataSerializerRegistry {
    /// Serializer entries in registration order (index = ID).
    entries: Vec<EntityDataSerializerEntry>,
    /// Map from name to ID for fast lookup.
    name_to_id: FxHashMap<&'static str, i32>,
    /// Whether the registry has been frozen (no more registrations allowed).
    frozen: bool,
}

impl EntityDataSerializerRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            name_to_id: FxHashMap::default(),
            frozen: false,
        }
    }

    /// Register a serializer with its writer function. ID is determined by registration order.
    ///
    /// # Panics
    /// Panics if the registry has been frozen or if the name is already registered.
    pub fn register(&mut self, name: &'static str, writer: EntityDataWriter) {
        assert!(!self.frozen, "Cannot register after freezing");
        assert!(
            !self.name_to_id.contains_key(name),
            "Serializer '{}' already registered",
            name
        );

        let id = self.entries.len() as i32;
        self.entries
            .push(EntityDataSerializerEntry { name, writer });
        self.name_to_id.insert(name, id);
    }

    /// Get the ID for a serializer by name.
    pub fn get_id(&self, name: &str) -> Option<i32> {
        self.name_to_id.get(name).copied()
    }

    /// Get the name for a serializer by ID.
    pub fn get_name(&self, id: i32) -> Option<&'static str> {
        self.entries.get(id as usize).map(|e| e.name)
    }

    /// Get the entry for a serializer by ID.
    pub fn get_entry(&self, id: i32) -> Option<&EntityDataSerializerEntry> {
        self.entries.get(id as usize)
    }

    /// Get the writer function for a serializer by ID.
    pub fn get_writer(&self, id: i32) -> Option<EntityDataWriter> {
        self.entries.get(id as usize).map(|e| e.writer)
    }

    /// Returns the number of registered serializers.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if no serializers are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Freeze the registry, preventing further registrations.
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// Returns true if the registry has been frozen.
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

impl Default for EntityDataSerializerRegistry {
    fn default() -> Self {
        Self::new()
    }
}
