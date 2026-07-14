//! Data component registry and storage types.
//!
//! This module provides:
//! - [`DataComponentRegistry`] - Registry of all component types with their serialization functions
//! - [`DataComponentMap`] - Storage for component values on items/entities
//! - [`DataComponentPatch`] - Diff representation for network/storage
//! - [`DataComponentType`] - Type-safe handle for accessing components

use rustc_hash::FxHashMap;
use simdnbt::{
    FromNbtTag, ToNbtTag,
    borrow::{NbtTag as BorrowedNbtTag, read_tag},
    owned::{NbtCompound, NbtTag as OwnedNbtTag},
};
use std::{
    fmt::Debug,
    io::{Cursor, Result, Write},
    marker::PhantomData,
};

use steel_utils::{
    Identifier,
    codec::VarInt,
    serial::{ReadFrom, WriteTo},
};

use super::component_data::{Component, ComponentData, ComponentDataDiscriminant};
use super::components::{ItemAttributeModifiers, ItemEnchantments};
use super::vanilla_components::{
    ATTRIBUTE_MODIFIERS, BREAK_SOUND, ENCHANTMENTS, LORE, MAX_STACK_SIZE, RARITY, REPAIR_COST,
    TOOLTIP_DISPLAY,
};

/// A typed handle for a data component.
///
/// This provides compile-time type safety when getting/setting components.
/// The actual storage uses [`ComponentData`] for ABI stability.
///
/// # Example
/// ```ignore
/// pub const DAMAGE: DataComponentType<Damage> =
///     DataComponentType::new(Identifier::vanilla_static("damage"));
///
/// // Type-safe access
/// let damage: Option<Damage> = components.get(DAMAGE);
/// components.set(DAMAGE, Damage(10));
/// ```
pub struct DataComponentType<T> {
    pub key: Identifier,
    _phantom: PhantomData<T>,
}

impl<T> Clone for DataComponentType<T> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<T> DataComponentType<T> {
    #[must_use]
    pub const fn new(key: Identifier) -> Self {
        Self {
            key,
            _phantom: PhantomData,
        }
    }
}

/// Reader function for deserializing a component from network format.
pub type NetworkReader = fn(&mut Cursor<&[u8]>) -> Result<ComponentData>;

/// Writer function for serializing a component to network format.
pub type NetworkWriter = fn(&ComponentData, &mut Vec<u8>) -> Result<()>;

/// Reader function for deserializing a component from NBT format.
pub type NbtReader = fn(BorrowedNbtTag) -> Option<ComponentData>;

/// Writer function for serializing a component to NBT format.
pub type NbtWriter = fn(&ComponentData) -> OwnedNbtTag;

/// Metadata for a registered component type.
///
/// Contains the component's key and all serialization functions needed
/// to read/write the component for network and persistent storage.
pub struct ComponentEntry {
    /// The component's identifier (e.g., "minecraft:damage")
    pub key: Identifier,
    /// Expected discriminant for this component type
    pub expected_discriminant: ComponentDataDiscriminant,
    /// Network protocol reader
    pub network_reader: NetworkReader,
    /// Network protocol writer
    pub network_writer: NetworkWriter,
    /// NBT storage reader
    pub nbt_reader: NbtReader,
    /// NBT storage writer
    pub nbt_writer: NbtWriter,
    persistent: bool,
}

impl ComponentEntry {
    /// Creates a new component entry with all serialization functions.
    #[must_use]
    pub fn new(
        key: Identifier,
        expected_discriminant: ComponentDataDiscriminant,
        network_reader: NetworkReader,
        network_writer: NetworkWriter,
        nbt_reader: NbtReader,
        nbt_writer: NbtWriter,
        persistent: bool,
    ) -> Self {
        Self {
            key,
            expected_discriminant,
            network_reader,
            network_writer,
            nbt_reader,
            nbt_writer,
            persistent,
        }
    }

    /// Validates that a `ComponentData` value matches the expected type for this component.
    ///
    /// Returns `true` if the data is valid for this component type, `false` otherwise.
    /// This prevents plugins from setting wrong types on vanilla components.
    #[must_use]
    pub fn validates(&self, data: &ComponentData) -> bool {
        data.discriminant() == self.expected_discriminant
    }

    /// Returns whether this component has a persistent storage codec.
    #[must_use]
    pub const fn is_persistent(&self) -> bool {
        self.persistent
    }

    /// Decodes an owned NBT value with this component's registered persistent codec.
    #[must_use]
    pub fn read_nbt_owned(&self, tag: &OwnedNbtTag) -> Option<ComponentData> {
        if !self.is_persistent() {
            return None;
        }
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        (self.nbt_reader)(borrowed.as_tag())
    }
}

pub type ComponentEntryRef = &'static ComponentEntry;

/// Registry of all data component types.
///
/// Stores metadata about each component type including how to serialize/deserialize
/// them for network and persistent storage.
pub struct DataComponentRegistry {
    /// Component entries indexed by network ID
    entries: Vec<ComponentEntryRef>,
    /// Map from component key to network ID
    by_key: FxHashMap<Identifier, usize>,
    /// Whether registration is still allowed
    allows_registering: bool,
}

impl Default for DataComponentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DataComponentRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers a vanilla component type.
    ///
    /// The component type `T` must implement the necessary serialization traits.
    /// This creates the appropriate reader/writer functions automatically.
    pub fn register<T>(
        &mut self,
        component: DataComponentType<T>,
        expected_discriminant: ComponentDataDiscriminant,
    ) where
        T: 'static + Component + WriteTo + ReadFrom + ToNbtTag + FromNbtTag,
    {
        self.register_with_persistence(component, expected_discriminant, true);
    }

    /// Registers a transient vanilla component type.
    ///
    /// Transient components have network data but no persistent component codec.
    pub fn register_transient<T>(
        &mut self,
        component: DataComponentType<T>,
        expected_discriminant: ComponentDataDiscriminant,
    ) where
        T: 'static + Component + WriteTo + ReadFrom + ToNbtTag + FromNbtTag,
    {
        self.register_with_persistence(component, expected_discriminant, false);
    }

    fn register_with_persistence<T>(
        &mut self,
        component: DataComponentType<T>,
        expected_discriminant: ComponentDataDiscriminant,
        persistent: bool,
    ) where
        T: 'static + Component + WriteTo + ReadFrom + ToNbtTag + FromNbtTag,
    {
        assert!(
            self.allows_registering,
            "Cannot register data components after the registry has been frozen"
        );

        // Create reader/writer functions that handle the ComponentData conversion
        fn make_network_reader<T>() -> NetworkReader
        where
            T: 'static + Component + ReadFrom,
        {
            |cursor| {
                let value = T::read(cursor)?;
                Ok(value.into_data())
            }
        }

        fn make_network_writer<T>() -> NetworkWriter
        where
            T: 'static + Component + WriteTo,
        {
            |data, writer| {
                if let Some(value) = T::from_data_ref(data) {
                    value.write(writer)
                } else {
                    Err(std::io::Error::other("Component type mismatch"))
                }
            }
        }

        fn make_nbt_reader<T>() -> NbtReader
        where
            T: 'static + Component + FromNbtTag,
        {
            |tag| {
                let value = T::from_nbt_tag(tag)?;
                Some(value.into_data())
            }
        }

        fn make_nbt_writer<T>() -> NbtWriter
        where
            T: 'static + Component + ToNbtTag + Clone,
        {
            |data| {
                if let Some(value) = T::from_data_ref(data) {
                    value.clone().to_nbt_tag()
                } else {
                    // Fallback: empty compound
                    OwnedNbtTag::Compound(NbtCompound::new())
                }
            }
        }

        let entry = Box::leak(Box::new(ComponentEntry::new(
            component.key.clone(),
            expected_discriminant,
            make_network_reader::<T>(),
            make_network_writer::<T>(),
            make_nbt_reader::<T>(),
            make_nbt_writer::<T>(),
            persistent,
        )));

        let id = self.entries.len();
        self.by_key.insert(component.key.clone(), id);
        self.entries.push(entry);
    }

    /// Registers a component with custom network reader/writer functions.
    ///
    /// Use this when the default `WriteTo`/`ReadFrom` implementations don't match
    /// the network encoding (e.g., VarInt-encoded i32 components).
    /// NBT serialization still uses the type's `ToNbtTag`/`FromNbtTag` impls.
    pub fn register_custom_network<T>(
        &mut self,
        component: DataComponentType<T>,
        expected_discriminant: ComponentDataDiscriminant,
        network_reader: NetworkReader,
        network_writer: NetworkWriter,
    ) where
        T: 'static + Component + ToNbtTag + FromNbtTag,
    {
        assert!(
            self.allows_registering,
            "Cannot register data components after the registry has been frozen"
        );

        fn make_nbt_reader<T>() -> NbtReader
        where
            T: 'static + Component + FromNbtTag,
        {
            |tag| {
                let value = T::from_nbt_tag(tag)?;
                Some(value.into_data())
            }
        }

        fn make_nbt_writer<T>() -> NbtWriter
        where
            T: 'static + Component + ToNbtTag + Clone,
        {
            |data| {
                if let Some(value) = T::from_data_ref(data) {
                    value.clone().to_nbt_tag()
                } else {
                    OwnedNbtTag::Compound(NbtCompound::new())
                }
            }
        }

        let entry = Box::leak(Box::new(ComponentEntry::new(
            component.key.clone(),
            expected_discriminant,
            network_reader,
            network_writer,
            make_nbt_reader::<T>(),
            make_nbt_writer::<T>(),
            true,
        )));

        let id = self.entries.len();
        self.by_key.insert(component.key.clone(), id);
        self.entries.push(entry);
    }

    /// Registers a component with explicit network and persistent codecs.
    pub fn register_with_codecs(
        &mut self,
        key: Identifier,
        expected_discriminant: ComponentDataDiscriminant,
        network_reader: NetworkReader,
        network_writer: NetworkWriter,
        nbt_reader: NbtReader,
        nbt_writer: NbtWriter,
    ) -> usize {
        self.register_with_persistence_codecs(
            key,
            expected_discriminant,
            network_reader,
            network_writer,
            nbt_reader,
            nbt_writer,
            true,
        )
    }

    /// Registers a transient component with explicit network codecs.
    pub fn register_transient_with_codecs(
        &mut self,
        key: Identifier,
        expected_discriminant: ComponentDataDiscriminant,
        network_reader: NetworkReader,
        network_writer: NetworkWriter,
        nbt_reader: NbtReader,
        nbt_writer: NbtWriter,
    ) -> usize {
        self.register_with_persistence_codecs(
            key,
            expected_discriminant,
            network_reader,
            network_writer,
            nbt_reader,
            nbt_writer,
            false,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "component registration keeps the four codec functions explicit"
    )]
    fn register_with_persistence_codecs(
        &mut self,
        key: Identifier,
        expected_discriminant: ComponentDataDiscriminant,
        network_reader: NetworkReader,
        network_writer: NetworkWriter,
        nbt_reader: NbtReader,
        nbt_writer: NbtWriter,
        persistent: bool,
    ) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register data components after the registry has been frozen"
        );

        let entry = Box::leak(Box::new(ComponentEntry::new(
            key.clone(),
            expected_discriminant,
            network_reader,
            network_writer,
            nbt_reader,
            nbt_writer,
            persistent,
        )));

        let id = self.entries.len();
        self.by_key.insert(key, id);
        self.entries.push(entry);
        id
    }

    /// Gets the network ID for a component type.
    #[must_use]
    pub fn get_id<T>(&self, component: DataComponentType<T>) -> Option<usize> {
        self.by_key.get(&component.key).copied()
    }

    /// Gets the component key by network ID.
    #[must_use]
    pub fn get_key_by_id(&self, id: usize) -> Option<&Identifier> {
        self.entries.get(id).map(|e| &e.key)
    }
}

crate::impl_registry!(
    DataComponentRegistry,
    ComponentEntry,
    entries,
    by_key,
    data_components
);

/// Storage for component values.
///
/// Maps component keys to their values. Used on items to store their data components.
#[derive(Debug, Clone)]
pub struct DataComponentMap {
    map: FxHashMap<Identifier, ComponentData>,
}

impl Default for DataComponentMap {
    fn default() -> Self {
        Self::new()
    }
}

impl DataComponentMap {
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: FxHashMap::default(),
        }
    }

    /// Creates a map with common item components pre-populated.
    #[must_use]
    pub fn common_item_components() -> Self {
        let mut map = FxHashMap::default();
        map.insert(MAX_STACK_SIZE.key.clone(), ComponentData::I32(64));
        map.insert(LORE.key.clone(), ComponentData::Todo);
        map.insert(
            ENCHANTMENTS.key.clone(),
            ComponentData::Enchantments(ItemEnchantments::empty()),
        );
        map.insert(REPAIR_COST.key.clone(), ComponentData::I32(0));
        map.insert(
            ATTRIBUTE_MODIFIERS.key.clone(),
            ComponentData::AttributeModifiers(ItemAttributeModifiers::empty()),
        );
        map.insert(RARITY.key.clone(), ComponentData::Todo);
        map.insert(BREAK_SOUND.key.clone(), ComponentData::Todo);
        map.insert(TOOLTIP_DISPLAY.key.clone(), ComponentData::Todo);
        Self { map }
    }

    /// Sets a component value (builder pattern).
    #[must_use]
    pub fn builder_set<T: Component>(
        mut self,
        component: DataComponentType<T>,
        value: Option<T>,
    ) -> Self {
        self.set(component, value);
        self
    }

    /// Sets a component value, or removes it if `None`.
    pub fn set<T: Component>(&mut self, component: DataComponentType<T>, value: Option<T>) {
        if let Some(v) = value {
            self.map.insert(component.key.clone(), v.into_data());
        } else {
            self.map.remove(&component.key);
        }
    }

    /// Gets a component value by type.
    #[must_use]
    pub fn get<T: Component>(&self, component: DataComponentType<T>) -> Option<T> {
        let data = self.map.get(&component.key)?;
        T::from_data(data.clone())
    }

    /// Gets a reference to a component value.
    #[must_use]
    pub fn get_ref<T: Component>(&self, component: DataComponentType<T>) -> Option<&T> {
        let data = self.map.get(&component.key)?;
        T::from_data_ref(data)
    }

    /// Checks if a component is present.
    #[must_use]
    pub fn has<T>(&self, component: DataComponentType<T>) -> bool {
        self.map.contains_key(&component.key)
    }

    /// Returns the number of components.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns true if empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Iterates over component keys.
    pub fn keys(&self) -> impl Iterator<Item = &Identifier> {
        self.map.keys()
    }

    /// Gets raw component data by key (for plugin use).
    #[must_use]
    pub fn get_raw(&self, key: &Identifier) -> Option<&ComponentData> {
        self.map.get(key)
    }

    /// Sets raw component data (for plugin use).
    ///
    /// Returns `true` if the data was set successfully, or `false` if the key is
    /// unregistered or the data type does not match it.
    ///
    /// This prevents plugins from setting invalid types on vanilla components.
    pub fn set_raw(&mut self, key: Identifier, data: ComponentData) -> bool {
        use crate::{REGISTRY, RegistryExt};

        let Some(entry) = REGISTRY.data_components.by_key(&key) else {
            return false;
        };
        if !entry.validates(&data) {
            return false;
        }

        self.map.insert(key, data);
        true
    }

    /// Removes a component by key.
    pub fn remove(&mut self, key: &Identifier) -> Option<ComponentData> {
        self.map.remove(key)
    }
}

/// Entry in a component patch.
#[derive(Debug, Clone)]
#[expect(
    clippy::large_enum_variant,
    reason = "component patches keep set values inline to avoid changing shared item component storage semantics"
)]
pub enum ComponentPatchEntry {
    /// Component is set to this value
    Set(ComponentData),
    /// Component is explicitly removed
    Removed,
}

impl PartialEq for ComponentPatchEntry {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Removed, Self::Removed) => true,
            (Self::Set(a), Self::Set(b)) => a == b,
            _ => false,
        }
    }
}

/// A patch representing modifications to a [`DataComponentMap`].
///
/// Stores differences from a prototype:
/// - Components that are added or overridden (`Set`)
/// - Components that are explicitly removed (`Removed`)
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DataComponentPatch {
    entries: FxHashMap<Identifier, ComponentPatchEntry>,
}

impl DataComponentPatch {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: FxHashMap::default(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Sets a component value in the patch.
    pub fn set<T: Component>(&mut self, component: DataComponentType<T>, value: T) {
        self.entries.insert(
            component.key.clone(),
            ComponentPatchEntry::Set(value.into_data()),
        );
    }

    /// Sets raw component data (for plugin use).
    ///
    /// Returns `true` if the data was set successfully, or `false` if the key is
    /// unregistered or the data type does not match it.
    ///
    /// This prevents plugins from setting invalid types on vanilla components.
    pub fn set_raw(&mut self, key: Identifier, data: ComponentData) -> bool {
        use crate::{REGISTRY, RegistryExt};

        let Some(entry) = REGISTRY.data_components.by_key(&key) else {
            return false;
        };
        if !entry.validates(&data) {
            return false;
        }

        self.entries.insert(key, ComponentPatchEntry::Set(data));
        true
    }

    /// Marks a component as removed.
    pub fn remove<T>(&mut self, component: DataComponentType<T>) {
        self.entries
            .insert(component.key.clone(), ComponentPatchEntry::Removed);
    }

    /// Marks a dynamically resolved component as removed.
    pub fn remove_raw(&mut self, key: Identifier) -> bool {
        use crate::{REGISTRY, RegistryExt};

        if REGISTRY.data_components.by_key(&key).is_none() {
            return false;
        }
        self.entries.insert(key, ComponentPatchEntry::Removed);
        true
    }

    /// Clears any patch entry for a component.
    pub fn clear<T>(&mut self, component: DataComponentType<T>) {
        self.entries.remove(&component.key);
    }

    /// Gets the patch entry for a key.
    #[must_use]
    pub fn get_entry(&self, key: &Identifier) -> Option<&ComponentPatchEntry> {
        self.entries.get(key)
    }

    /// Checks if a component is marked as removed.
    #[must_use]
    pub fn is_removed(&self, key: &Identifier) -> bool {
        matches!(self.entries.get(key), Some(ComponentPatchEntry::Removed))
    }

    /// Counts set entries.
    #[must_use]
    pub fn count_set(&self) -> usize {
        self.entries
            .values()
            .filter(|e| matches!(e, ComponentPatchEntry::Set(_)))
            .count()
    }

    /// Counts removed entries.
    #[must_use]
    pub fn count_removed(&self) -> usize {
        self.entries
            .values()
            .filter(|e| matches!(e, ComponentPatchEntry::Removed))
            .count()
    }

    /// Iterates over all entries.
    pub fn iter(&self) -> impl Iterator<Item = (&Identifier, &ComponentPatchEntry)> {
        self.entries.iter()
    }

    /// Iterates over removed component keys.
    pub fn iter_removed(&self) -> impl Iterator<Item = &Identifier> {
        self.entries.iter().filter_map(|(k, v)| {
            if matches!(v, ComponentPatchEntry::Removed) {
                Some(k)
            } else {
                None
            }
        })
    }

    /// Converts this component patch to NBT without consuming it.
    #[must_use]
    pub fn to_nbt_tag_ref(&self) -> OwnedNbtTag {
        use crate::{REGISTRY, RegistryExt};

        let mut compound = NbtCompound::new();

        for (key, entry) in &self.entries {
            let Some(component) = REGISTRY.data_components.by_key(key) else {
                continue;
            };
            if !component.is_persistent() {
                continue;
            }
            match entry {
                ComponentPatchEntry::Set(data) => {
                    let nbt = (component.nbt_writer)(data);
                    compound.insert(key.to_string(), nbt);
                }
                ComponentPatchEntry::Removed => {
                    compound.insert(format!("!{key}"), NbtCompound::new());
                }
            }
        }

        OwnedNbtTag::Compound(compound)
    }
}

impl WriteTo for DataComponentPatch {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        use crate::{REGISTRY, RegistryExt};

        let mut added: Vec<(&Identifier, &ComponentData)> = Vec::new();
        let mut removed: Vec<&Identifier> = Vec::new();

        for (key, entry) in &self.entries {
            match entry {
                ComponentPatchEntry::Set(data) => added.push((key, data)),
                ComponentPatchEntry::Removed => removed.push(key),
            }
        }

        let added_count = i32::try_from(added.len())
            .map_err(|_| std::io::Error::other("Too many added data components"))?;
        let removed_count = i32::try_from(removed.len())
            .map_err(|_| std::io::Error::other("Too many removed data components"))?;
        VarInt(added_count).write(writer)?;
        VarInt(removed_count).write(writer)?;

        // Write added components
        for (key, data) in added {
            let id = REGISTRY
                .data_components
                .id_from_key(key)
                .ok_or_else(|| std::io::Error::other(format!("Unknown component key: {key:?}")))?;

            let entry = REGISTRY
                .data_components
                .by_id(id)
                .ok_or_else(|| std::io::Error::other(format!("No entry for component id: {id}")))?;

            VarInt(id as i32).write(writer)?;

            let mut buf = Vec::new();
            (entry.network_writer)(data, &mut buf)?;
            writer.write_all(&buf)?;
        }

        // Write removed component IDs
        for key in removed {
            let id = REGISTRY
                .data_components
                .id_from_key(key)
                .ok_or_else(|| std::io::Error::other(format!("Unknown component key: {key:?}")))?;
            VarInt(id as i32).write(writer)?;
        }

        Ok(())
    }
}

impl ReadFrom for DataComponentPatch {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        use crate::{REGISTRY, RegistryExt};

        let added_count = read_component_count(data, "added")?;
        let removed_count = read_component_count(data, "removed")?;

        log::info!("Reading DataComponentPatch: added={added_count}, removed={removed_count}");

        let mut patch = Self::new();

        // Read added components
        for i in 0..added_count {
            let pos_before = data.position();
            let type_id = read_non_negative_varint(data, "component type id")?;

            let key = REGISTRY
                .data_components
                .get_key_by_id(type_id)
                .ok_or_else(|| {
                    std::io::Error::other(format!("Unknown component type ID: {type_id}"))
                })?
                .clone();

            log::info!("  [{i}] Reading component {key} (id={type_id}) at pos {pos_before}");

            let entry = REGISTRY
                .data_components
                .by_id(type_id)
                .ok_or_else(|| std::io::Error::other(format!("No entry for component: {key}")))?;

            let component_data = (entry.network_reader)(data).map_err(|e| {
                log::error!("    Failed to read component {key}: {e}");
                e
            })?;

            let pos_after = data.position();
            log::info!("    Read {} bytes for {key}", pos_after - pos_before);

            patch
                .entries
                .insert(key, ComponentPatchEntry::Set(component_data));
        }

        // Read removed component IDs
        for _ in 0..removed_count {
            let type_id = read_non_negative_varint(data, "component type id")?;

            let key = REGISTRY
                .data_components
                .get_key_by_id(type_id)
                .ok_or_else(|| {
                    std::io::Error::other(format!("Unknown component type ID: {type_id}"))
                })?
                .clone();

            patch.entries.insert(key, ComponentPatchEntry::Removed);
        }

        Ok(patch)
    }
}

impl DataComponentPatch {
    /// Reads a patch where each component value is prefixed with a `VarInt` byte length.
    ///
    /// Vanilla uses this for untrusted client packets (e.g., creative mode slot)
    /// via `DataComponentPatch.DELIMITED_STREAM_CODEC`.
    pub fn read_delimited(data: &mut Cursor<&[u8]>) -> Result<Self> {
        use crate::{REGISTRY, RegistryExt};
        use std::io::Read;

        let added_count = read_component_count(data, "added")?;
        let removed_count = read_component_count(data, "removed")?;

        const MAX_COMPONENTS: usize = 65_536;
        const MAX_COMPONENT_BYTES: usize = 2 * 1024 * 1024;

        if added_count.saturating_add(removed_count) > MAX_COMPONENTS {
            return Err(std::io::Error::other(format!(
                "Component patch too large: {added_count} added + {removed_count} removed > {MAX_COMPONENTS}"
            )));
        }

        let mut patch = Self::new();

        for _ in 0..added_count {
            let type_id = read_non_negative_varint(data, "component type id")?;
            let byte_len = read_non_negative_varint(data, "component byte length")?;

            if byte_len > MAX_COMPONENT_BYTES {
                return Err(std::io::Error::other(format!(
                    "Component data too large: {byte_len} bytes > {MAX_COMPONENT_BYTES}"
                )));
            }

            let key = REGISTRY
                .data_components
                .get_key_by_id(type_id)
                .ok_or_else(|| {
                    std::io::Error::other(format!("Unknown component type ID: {type_id}"))
                })?
                .clone();

            let entry = REGISTRY
                .data_components
                .by_id(type_id)
                .ok_or_else(|| std::io::Error::other(format!("No entry for component: {key}")))?;

            // Read the component bytes into a sub-buffer
            let mut buf = vec![0u8; byte_len];
            data.read_exact(&mut buf)?;

            let mut sub_cursor = Cursor::new(buf.as_slice());
            let component_data = (entry.network_reader)(&mut sub_cursor)?;
            patch
                .entries
                .insert(key, ComponentPatchEntry::Set(component_data));
        }

        for _ in 0..removed_count {
            let type_id = read_non_negative_varint(data, "component type id")?;
            let key = REGISTRY
                .data_components
                .get_key_by_id(type_id)
                .ok_or_else(|| {
                    std::io::Error::other(format!("Unknown component type ID: {type_id}"))
                })?
                .clone();
            patch.entries.insert(key, ComponentPatchEntry::Removed);
        }

        Ok(patch)
    }
}

fn read_component_count(data: &mut Cursor<&[u8]>, kind: &str) -> Result<usize> {
    read_non_negative_varint(data, &format!("{kind} component count"))
}

fn read_non_negative_varint(data: &mut Cursor<&[u8]>, name: &str) -> Result<usize> {
    let value = VarInt::read(data)?.0;
    usize::try_from(value).map_err(|_| std::io::Error::other(format!("Negative {name}: {value}")))
}

impl ToNbtTag for DataComponentPatch {
    fn to_nbt_tag(self) -> OwnedNbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for DataComponentPatch {
    fn from_nbt_tag(tag: BorrowedNbtTag) -> Option<Self> {
        use crate::{REGISTRY, RegistryExt};

        let compound = tag.compound()?;
        let mut patch = Self::new();

        for (key, value) in compound.iter() {
            let key_str = key.to_str();

            if let Some(stripped) = key_str.strip_prefix('!') {
                let id = stripped.parse::<Identifier>().ok()?;
                let entry = REGISTRY.data_components.by_key(&id)?;
                if !entry.is_persistent() || value.compound().is_none() {
                    return None;
                }
                patch.entries.insert(id, ComponentPatchEntry::Removed);
            } else {
                let id = key_str.parse::<Identifier>().ok()?;
                let entry = REGISTRY.data_components.by_key(&id)?;
                if !entry.is_persistent() {
                    return None;
                }
                let component_data = (entry.nbt_reader)(value)?;
                patch
                    .entries
                    .insert(id, ComponentPatchEntry::Set(component_data));
            }
        }

        Some(patch)
    }
}

/// Attempts to extract a typed component from `ComponentData`.
#[must_use]
pub fn component_try_into<T: Component>(
    data: &ComponentData,
    _component: DataComponentType<T>,
) -> Option<&T> {
    T::from_data_ref(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        data_components::vanilla_components::{
            ADDITIONAL_TRADE_COST, CREATIVE_SLOT_LOCK, MAP_POST_PROCESSING, MAX_STACK_SIZE,
        },
        test_support::init_test_registry,
    };
    use simdnbt::borrow::{NbtTag as BorrowedNbtTag, read_tag};

    fn with_borrowed_tag<R>(
        tag: OwnedNbtTag,
        visitor: impl FnOnce(BorrowedNbtTag<'_, '_>) -> R,
    ) -> R {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("owned test tag should parse");
        visitor(borrowed.as_tag())
    }

    fn parse_patch(tag: OwnedNbtTag) -> Option<DataComponentPatch> {
        with_borrowed_tag(tag, DataComponentPatch::from_nbt_tag)
    }

    #[test]
    fn persistent_patch_nbt_omits_transient_components() {
        init_test_registry();
        let mut patch = DataComponentPatch::new();
        patch.set(MAX_STACK_SIZE, 16);
        patch.set(CREATIVE_SLOT_LOCK, ());
        patch.remove(ADDITIONAL_TRADE_COST);
        patch.set(MAP_POST_PROCESSING, ());

        let OwnedNbtTag::Compound(compound) = patch.to_nbt_tag_ref() else {
            panic!("component patch should serialize as a compound");
        };
        assert!(compound.get("minecraft:max_stack_size").is_some());
        assert!(compound.get("minecraft:creative_slot_lock").is_none());
        assert!(compound.get("!minecraft:additional_trade_cost").is_none());
        assert!(compound.get("minecraft:map_post_processing").is_none());
    }

    #[test]
    fn persistent_patch_decode_fails_on_invalid_entries() {
        init_test_registry();

        let mut valid = NbtCompound::new();
        valid.insert("minecraft:max_stack_size", OwnedNbtTag::Double(16.9));
        let patch = parse_patch(OwnedNbtTag::Compound(valid))
            .expect("numeric component value should use codec coercion");
        assert_eq!(
            patch.get_entry(&MAX_STACK_SIZE.key),
            Some(&ComponentPatchEntry::Set(ComponentData::I32(16)))
        );

        let mut out_of_range = NbtCompound::new();
        out_of_range.insert("minecraft:max_stack_size", 0);
        assert!(parse_patch(OwnedNbtTag::Compound(out_of_range)).is_none());

        let mut unknown = NbtCompound::new();
        unknown.insert("minecraft:not_a_component", NbtCompound::new());
        assert!(parse_patch(OwnedNbtTag::Compound(unknown)).is_none());

        let mut malformed_removal = NbtCompound::new();
        malformed_removal.insert("!minecraft:max_stack_size", 1);
        assert!(parse_patch(OwnedNbtTag::Compound(malformed_removal)).is_none());
    }
}
