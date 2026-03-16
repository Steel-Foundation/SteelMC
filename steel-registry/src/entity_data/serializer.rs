//! Entity data serializer registry.
//!
//! This registry maps serializer keys to IDs and stores writer functions.
//! Registration order determines the serializer ID used in the network protocol,
//! matching vanilla's `EntityDataSerializers.java`.

use rustc_hash::FxHashMap;
use std::io;
use steel_utils::Identifier;

use super::EntityData;

/// Writer function for serializing entity data to network format.
///
/// Takes a reference to the [`EntityData`] value and writes it to the buffer.
/// Returns an error if the value doesn't match the expected serializer type.
pub type EntityDataWriter = fn(&EntityData, &mut Vec<u8>) -> io::Result<()>;

/// Entry for a registered entity data serializer.
pub struct EntityDataSerializerEntry {
    /// The serializer's identifier (e.g., "minecraft:byte").
    pub key: Identifier,
    /// The writer function for this serializer.
    pub writer: EntityDataWriter,
}

pub type EntityDataSerializerEntryRef = &'static EntityDataSerializerEntry;

/// Registry of entity data serializers.
///
/// The serializer ID is determined by registration order, which must match
/// vanilla's `EntityDataSerializers.java` exactly.
pub struct EntityDataSerializerRegistry {
    /// Serializer entries in registration order (index = ID).
    entries_by_id: Vec<EntityDataSerializerEntryRef>,
    /// Map from key to ID for fast lookup.
    entries_by_key: FxHashMap<Identifier, usize>,
    /// Whether registration is still allowed.
    allows_registering: bool,
}

impl EntityDataSerializerRegistry {
    /// Create a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries_by_id: Vec::new(),
            entries_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Register a serializer with its writer function. ID is determined by registration order.
    ///
    /// # Panics
    /// Panics if the registry has been frozen or if the key is already registered.
    pub fn register(&mut self, key: Identifier, writer: EntityDataWriter) {
        assert!(
            self.allows_registering,
            "Cannot register entity data serializers after the registry has been frozen"
        );
        assert!(
            !self.entries_by_key.contains_key(&key),
            "Serializer '{key}' already registered",
        );

        let entry = Box::leak(Box::new(EntityDataSerializerEntry {
            key: key.clone(),
            writer,
        }));
        let id = self.entries_by_id.len();
        self.entries_by_id.push(entry);
        self.entries_by_key.insert(key, id);
    }

    /// Get the writer function for a serializer by protocol ID.
    #[must_use]
    pub fn get_writer(&self, id: i32) -> Option<EntityDataWriter> {
        self.entries_by_id.get(id as usize).map(|e| e.writer)
    }
}

impl Default for EntityDataSerializerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    EntityDataSerializerRegistry,
    EntityDataSerializerEntry,
    entries_by_id,
    entries_by_key,
    entity_data_serializers
);
