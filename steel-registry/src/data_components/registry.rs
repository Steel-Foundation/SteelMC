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
    DowncastType, DowncastTypeKey, Identifier,
    codec::VarInt,
    hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries},
    serial::{ReadFrom, WriteTo},
};
use text_components::EmbeddedNbtCodec;

use super::component_data::{Component, ComponentData};
use super::components::{
    ItemAttributeModifiers, ItemEnchantments, ItemLore, Rarity, SwingAnimation, TooltipDisplay,
    UseEffects,
};
use super::vanilla_components::{
    ATTRIBUTE_MODIFIERS, BREAK_SOUND, ENCHANTMENTS, LORE, MAX_STACK_SIZE, RARITY, REPAIR_COST,
    SWING_ANIMATION, TOOLTIP_DISPLAY, USE_EFFECTS,
};
use crate::{sound_event::SoundEventHolder, sound_events};

/// A typed handle for a data component.
///
/// This provides compile-time type safety when getting/setting components.
/// The actual storage uses keyed type erasure through [`ComponentData`].
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
///
/// Steel declares component handles alongside their registered codecs; external
/// callers cannot construct a handle for an existing key with a different type.
///
/// ```compile_fail
/// use steel_registry::data_components::DataComponentType;
/// use steel_utils::Identifier;
///
/// let _forged = DataComponentType::<bool>::new(Identifier::vanilla_static("max_damage"));
/// ```
pub struct DataComponentType<T> {
    pub(crate) key: Identifier,
    ignore_swap_animation: bool,
    _phantom: PhantomData<T>,
}

impl<T> Clone for DataComponentType<T> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            ignore_swap_animation: self.ignore_swap_animation,
            _phantom: PhantomData,
        }
    }
}

impl<T> DataComponentType<T> {
    #[must_use]
    pub(crate) const fn new(key: Identifier) -> Self {
        Self {
            key,
            ignore_swap_animation: false,
            _phantom: PhantomData,
        }
    }

    /// Creates a component type whose changes do not restart the held-item swap animation.
    #[must_use]
    pub(crate) const fn new_ignoring_swap_animation(key: Identifier) -> Self {
        Self {
            key,
            ignore_swap_animation: true,
            _phantom: PhantomData,
        }
    }

    /// Returns whether this component is ignored when comparing held items for swap animation.
    #[must_use]
    pub const fn ignore_swap_animation(&self) -> bool {
        self.ignore_swap_animation
    }

    /// Returns this component type's registry key.
    #[must_use]
    pub const fn key(&self) -> &Identifier {
        &self.key
    }
}

/// Reader function for deserializing a component from network format.
pub type NetworkReader = fn(&mut Cursor<&[u8]>) -> Result<ComponentData>;

/// Writer function for serializing a component to network format.
pub type NetworkWriter = fn(&ComponentData, &mut Vec<u8>) -> Result<()>;

/// Reader function for deserializing a component from NBT format.
pub type NbtReader = fn(BorrowedNbtTag) -> Option<ComponentData>;

/// Writer function for serializing a component to NBT format.
pub type NbtWriter = fn(&ComponentData) -> Result<OwnedNbtTag>;

/// Function for hashing a component through its persistent codec shape.
type ComponentHash = fn(&ComponentData) -> Result<i32>;
type ComponentValidator = fn(&ComponentData) -> Result<()>;
type PersistentCodecFns = (
    NbtReader,
    NbtWriter,
    ComponentHash,
    Option<ComponentValidator>,
);

/// Additional source-value validation required before persistent encoding.
pub(crate) trait ValidatePersistentComponent {
    fn validate_persistent(&self) -> Result<()>;
}

fn hash_component<T: DowncastType + HashComponent>(data: &ComponentData) -> Result<i32> {
    let Some(value) = data.downcast_ref::<T>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    Ok(value.compute_hash())
}

fn validate_component<T: DowncastType + ValidatePersistentComponent>(
    data: &ComponentData,
) -> Result<()> {
    let Some(value) = data.downcast_ref::<T>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    value.validate_persistent()
}

fn read_typed_network<T: Component + ReadFrom>(
    cursor: &mut Cursor<&[u8]>,
) -> Result<ComponentData> {
    Ok(ComponentData::new(T::read(cursor)?))
}

fn write_typed_network<T: DowncastType + WriteTo>(
    data: &ComponentData,
    writer: &mut Vec<u8>,
) -> Result<()> {
    let Some(value) = data.downcast_ref::<T>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    value.write(writer)
}

fn read_typed_nbt<T: Component + FromNbtTag>(tag: BorrowedNbtTag) -> Option<ComponentData> {
    T::from_nbt_tag(tag).map(ComponentData::new)
}

fn write_typed_nbt<T: DowncastType + ToNbtTag + Clone>(
    data: &ComponentData,
) -> Result<OwnedNbtTag> {
    let Some(value) = data.downcast_ref::<T>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    Ok(value.clone().to_nbt_tag())
}

struct NetworkCodecs {
    reader: NetworkReader,
    writer: NetworkWriter,
}

struct PersistentCodecs {
    reader: NbtReader,
    writer: NbtWriter,
    hash: ComponentHash,
    validator: Option<fn(&ComponentData) -> Result<()>>,
}

struct ComponentCodecs {
    expected_type_key: DowncastTypeKey,
    network: NetworkCodecs,
    persistent: Option<PersistentCodecs>,
}

/// Metadata for a registered component type.
///
/// Contains the component's key and all serialization functions needed
/// to read/write the component for network and persistent storage.
pub struct ComponentEntry {
    /// The component's identifier (e.g., "minecraft:damage")
    pub key: Identifier,
    codecs: ComponentCodecs,
    ignore_swap_animation: bool,
}

impl ComponentEntry {
    #[must_use]
    fn implemented(
        key: Identifier,
        expected_type_key: DowncastTypeKey,
        network_reader: NetworkReader,
        network_writer: NetworkWriter,
        persistent_codecs: Option<PersistentCodecFns>,
        ignore_swap_animation: bool,
    ) -> Self {
        Self {
            key,
            codecs: ComponentCodecs {
                expected_type_key,
                network: NetworkCodecs {
                    reader: network_reader,
                    writer: network_writer,
                },
                persistent: persistent_codecs.map(|(reader, writer, hash, validator)| {
                    PersistentCodecs {
                        reader,
                        writer,
                        hash,
                        validator,
                    }
                }),
            },
            ignore_swap_animation,
        }
    }

    /// Validates that a `ComponentData` value matches the concrete type for this component.
    ///
    /// Returns `true` if the data is valid for this component type, `false` otherwise.
    /// This prevents plugins from setting wrong types on vanilla components.
    #[must_use]
    pub fn validates(&self, data: &ComponentData) -> bool {
        data.type_key() == self.codecs.expected_type_key
    }

    /// Decodes this component's network value.
    pub fn read_network(&self, data: &mut Cursor<&[u8]>) -> Result<ComponentData> {
        let ComponentCodecs {
            network,
            expected_type_key,
            ..
        } = &self.codecs;
        let value = (network.reader)(data)?;
        if value.type_key() != *expected_type_key {
            return Err(std::io::Error::other(format!(
                "Network codec returned the wrong value type for {}",
                self.key
            )));
        }
        Ok(value)
    }

    /// Encodes this component's network value after validating its concrete type.
    pub fn write_network(&self, data: &ComponentData, writer: &mut Vec<u8>) -> Result<()> {
        if !self.validates(data) {
            return Err(std::io::Error::other(format!(
                "Component value type does not match {}",
                self.key
            )));
        }
        (self.codecs.network.writer)(data, writer)
    }

    /// Decodes this component's persistent NBT value.
    #[must_use]
    pub fn read_nbt(&self, tag: BorrowedNbtTag) -> Option<ComponentData> {
        let Some(persistent) = &self.codecs.persistent else {
            return None;
        };
        let value = (persistent.reader)(tag)?;
        (value.type_key() == self.codecs.expected_type_key).then_some(value)
    }

    /// Encodes this component's persistent NBT value after validating its concrete type.
    pub fn write_nbt(&self, data: &ComponentData) -> Result<OwnedNbtTag> {
        if !self.validates(data) {
            return Err(std::io::Error::other(format!(
                "Component value type does not match {}",
                self.key
            )));
        }
        let Some(persistent) = &self.codecs.persistent else {
            return Err(std::io::Error::other(format!(
                "Transient component {} has no persistent codec",
                self.key
            )));
        };
        (persistent.writer)(data)
    }

    /// Checks that a value accepted by the stream codec is also accepted by
    /// the persistent codec.
    pub fn validate_persistent_encoding(&self, data: &ComponentData) -> Result<OwnedNbtTag> {
        if let Some(validator) = self
            .codecs
            .persistent
            .as_ref()
            .and_then(|persistent| persistent.validator)
        {
            validator(data)?;
        }
        let tag = self.write_nbt(data)?;
        if self.read_nbt_owned(&tag).is_none() {
            return Err(std::io::Error::other(format!(
                "Persistent codec for component {} rejected its encoded value",
                self.key
            )));
        }
        Ok(tag)
    }

    /// Computes the vanilla `HashOps` value through this component's persistent codec.
    pub fn compute_hash(&self, data: &ComponentData) -> Result<i32> {
        if !self.validates(data) {
            return Err(std::io::Error::other(format!(
                "Component value type does not match {}",
                self.key
            )));
        }
        if !self.is_persistent() {
            return Err(std::io::Error::other(format!(
                "Transient component {} has no persistent hash codec",
                self.key
            )));
        }
        let Some(persistent) = &self.codecs.persistent else {
            return Err(std::io::Error::other(format!(
                "Transient component {} has no persistent hash codec",
                self.key
            )));
        };
        self.validate_persistent_encoding(data)?;
        (persistent.hash)(data)
    }

    /// Returns whether vanilla defines this as a persistent component.
    #[must_use]
    pub const fn is_persistent(&self) -> bool {
        self.codecs.persistent.is_some()
    }

    /// Returns whether changes to this component are ignored for held-item swap animation.
    #[must_use]
    pub const fn ignore_swap_animation(&self) -> bool {
        self.ignore_swap_animation
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
        self.read_nbt(borrowed.as_tag())
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

impl DataComponentRegistry {
    #[must_use]
    pub(crate) fn new() -> Self {
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
    pub(crate) fn register<T>(&mut self, component: DataComponentType<T>)
    where
        T: Component
            + DowncastType
            + Clone
            + WriteTo
            + ReadFrom
            + ToNbtTag
            + FromNbtTag
            + HashComponent,
    {
        self.register_persistent(component);
    }

    /// Registers a transient vanilla component type.
    ///
    /// Transient components have network data but no persistent component codec.
    pub(crate) fn register_transient<T>(&mut self, component: DataComponentType<T>)
    where
        T: Component + DowncastType + WriteTo + ReadFrom,
    {
        self.register_implemented(
            component,
            read_typed_network::<T>,
            write_typed_network::<T>,
            None,
        );
    }

    fn register_persistent<T>(&mut self, component: DataComponentType<T>)
    where
        T: Component
            + DowncastType
            + Clone
            + WriteTo
            + ReadFrom
            + ToNbtTag
            + FromNbtTag
            + HashComponent,
    {
        self.register_implemented(
            component,
            read_typed_network::<T>,
            write_typed_network::<T>,
            Some((
                read_typed_nbt::<T>,
                write_typed_nbt::<T>,
                hash_component::<T>,
                None,
            )),
        );
    }

    pub(crate) fn register_validated<T>(&mut self, component: DataComponentType<T>)
    where
        T: Component
            + DowncastType
            + Clone
            + WriteTo
            + ReadFrom
            + ToNbtTag
            + FromNbtTag
            + HashComponent
            + ValidatePersistentComponent,
    {
        self.register_implemented(
            component,
            read_typed_network::<T>,
            write_typed_network::<T>,
            Some((
                read_typed_nbt::<T>,
                write_typed_nbt::<T>,
                hash_component::<T>,
                Some(validate_component::<T>),
            )),
        );
    }

    /// Registers a component with custom network reader/writer functions.
    ///
    /// Use this when the default `WriteTo`/`ReadFrom` implementations don't match
    /// the network encoding (e.g., VarInt-encoded i32 components).
    /// NBT serialization still uses the type's `ToNbtTag`/`FromNbtTag` impls.
    pub(crate) fn register_custom_network<T>(
        &mut self,
        component: DataComponentType<T>,
        network_reader: NetworkReader,
        network_writer: NetworkWriter,
    ) where
        T: Component + DowncastType + Clone + ToNbtTag + FromNbtTag + HashComponent,
    {
        self.register_implemented(
            component,
            network_reader,
            network_writer,
            Some((
                read_typed_nbt::<T>,
                write_typed_nbt::<T>,
                hash_component::<T>,
                None,
            )),
        );
    }

    /// Registers a component with explicit network and persistent codecs.
    pub(crate) fn register_with_codecs<T: Component + DowncastType + HashComponent>(
        &mut self,
        component: DataComponentType<T>,
        network_reader: NetworkReader,
        network_writer: NetworkWriter,
        nbt_reader: NbtReader,
        nbt_writer: NbtWriter,
    ) -> usize {
        self.register_implemented(
            component,
            network_reader,
            network_writer,
            Some((nbt_reader, nbt_writer, hash_component::<T>, None)),
        )
    }

    /// Registers a transient component with explicit network codecs.
    pub(crate) fn register_transient_with_codecs<T: Component + DowncastType>(
        &mut self,
        component: DataComponentType<T>,
        network_reader: NetworkReader,
        network_writer: NetworkWriter,
    ) -> usize {
        self.register_implemented(component, network_reader, network_writer, None)
    }

    fn register_implemented<T: Component + DowncastType>(
        &mut self,
        component: DataComponentType<T>,
        network_reader: NetworkReader,
        network_writer: NetworkWriter,
        persistent_codecs: Option<PersistentCodecFns>,
    ) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register data components after the registry has been frozen"
        );

        let ignore_swap_animation = component.ignore_swap_animation();
        let key = component.key;
        assert!(
            !self.by_key.contains_key(&key),
            "Cannot register duplicate data component key {key}"
        );
        let entry = Box::leak(Box::new(ComponentEntry::implemented(
            key.clone(),
            T::TYPE_KEY,
            network_reader,
            network_writer,
            persistent_codecs,
            ignore_swap_animation,
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
        map.insert(MAX_STACK_SIZE.key.clone(), ComponentData::new(64_i32));
        map.insert(LORE.key.clone(), ComponentData::new(ItemLore::empty()));
        map.insert(
            ENCHANTMENTS.key.clone(),
            ComponentData::new(ItemEnchantments::empty()),
        );
        map.insert(REPAIR_COST.key.clone(), ComponentData::new(0_i32));
        map.insert(
            USE_EFFECTS.key.clone(),
            ComponentData::new(UseEffects::DEFAULT),
        );
        map.insert(
            ATTRIBUTE_MODIFIERS.key.clone(),
            ComponentData::new(ItemAttributeModifiers::empty()),
        );
        map.insert(RARITY.key.clone(), ComponentData::new(Rarity::Common));
        map.insert(
            BREAK_SOUND.key.clone(),
            ComponentData::new(SoundEventHolder::registry(&sound_events::ENTITY_ITEM_BREAK)),
        );
        map.insert(
            TOOLTIP_DISPLAY.key.clone(),
            ComponentData::new(TooltipDisplay::DEFAULT),
        );
        map.insert(
            SWING_ANIMATION.key.clone(),
            ComponentData::new(SwingAnimation::DEFAULT),
        );
        Self { map }
    }

    /// Sets a component value (builder pattern).
    #[must_use]
    pub fn builder_set<T: Component + DowncastType>(
        mut self,
        component: DataComponentType<T>,
        value: Option<T>,
    ) -> Self {
        self.set(component, value);
        self
    }

    /// Sets a component value, or removes it if `None`.
    pub fn set<T: Component + DowncastType>(
        &mut self,
        component: DataComponentType<T>,
        value: Option<T>,
    ) {
        if let Some(v) = value {
            self.map
                .insert(component.key.clone(), ComponentData::new(v));
        } else {
            self.map.remove(&component.key);
        }
    }

    /// Gets a component value by type.
    #[must_use]
    pub fn get<T: Component + DowncastType + Clone>(
        &self,
        component: DataComponentType<T>,
    ) -> Option<T> {
        let data = self.map.get(&component.key)?;
        data.downcast_ref::<T>().cloned()
    }

    /// Gets a reference to a component value.
    #[must_use]
    pub fn get_ref<T: Component + DowncastType>(
        &self,
        component: DataComponentType<T>,
    ) -> Option<&T> {
        let data = self.map.get(&component.key)?;
        data.downcast_ref::<T>()
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
    pub fn set<T: Component + DowncastType>(&mut self, component: DataComponentType<T>, value: T) {
        self.entries.insert(
            component.key.clone(),
            ComponentPatchEntry::Set(ComponentData::new(value)),
        );
    }

    pub(crate) fn set_component_data(&mut self, key: Identifier, data: ComponentData) {
        self.entries.insert(key, ComponentPatchEntry::Set(data));
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

    pub(crate) fn sanitize_against(&mut self, prototype: &DataComponentMap) {
        self.entries.retain(|key, entry| {
            let default = prototype.get_raw(key);
            match entry {
                ComponentPatchEntry::Set(value) => default != Some(value),
                ComponentPatchEntry::Removed => default.is_some(),
            }
        });
    }

    /// Computes Vanilla's `HashOps` value for the persistent patch codec.
    pub fn compute_persistent_hash(&self) -> Result<i32> {
        use crate::{REGISTRY, RegistryExt};

        let mut entries = Vec::new();
        for (key, patch_entry) in &self.entries {
            let Some(component) = REGISTRY.data_components.by_key(key) else {
                continue;
            };
            if !component.is_persistent() {
                continue;
            }

            let (encoded_key, value_hash) = match patch_entry {
                ComponentPatchEntry::Set(data) => (key.to_string(), component.compute_hash(data)?),
                ComponentPatchEntry::Removed => (format!("!{key}"), ().compute_hash()),
            };
            entries.push(hash_entry(encoded_key.compute_hash(), value_hash));
        }
        sort_map_entries(&mut entries);

        let mut hasher = ComponentHasher::new();
        hasher.start_map();
        for entry in &entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
        Ok(hasher.finish())
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

    fn encode_nbt(&self, validate: bool) -> (OwnedNbtTag, Vec<std::io::Error>) {
        use crate::{REGISTRY, RegistryExt};

        let mut compound = NbtCompound::new();
        let mut errors = Vec::new();

        for (key, entry) in &self.entries {
            let Some(component) = REGISTRY.data_components.by_key(key) else {
                continue;
            };
            if !component.is_persistent() {
                continue;
            }
            match entry {
                ComponentPatchEntry::Set(data) => {
                    let encoded = if validate {
                        component.validate_persistent_encoding(data)
                    } else {
                        component.write_nbt(data)
                    };
                    match encoded {
                        Ok(nbt) => {
                            compound.insert(key.to_string(), nbt);
                        }
                        Err(error) => errors.push(std::io::Error::other(format!(
                            "failed to encode component {key}: {error}"
                        ))),
                    }
                }
                ComponentPatchEntry::Removed => {
                    compound.insert(format!("!{key}"), NbtCompound::new());
                }
            }
        }

        (OwnedNbtTag::Compound(compound), errors)
    }

    /// Strictly encodes this component patch through its persistent codecs.
    ///
    /// This is the equivalent of Vanilla encoding an untrusted stack through
    /// `ItemStack.CODEC` before accepting it into server state.
    pub fn try_to_nbt_tag_ref(&self) -> Result<OwnedNbtTag> {
        let (tag, errors) = self.encode_nbt(true);
        match errors.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(tag),
        }
    }

    /// Converts this component patch to NBT without consuming it.
    ///
    /// Save-time encoding mirrors Vanilla's `TagValueOutput`: invalid fields
    /// are reported and omitted from the partial result rather than aborting
    /// the owner save.
    #[must_use]
    pub fn to_nbt_tag_ref(&self) -> OwnedNbtTag {
        let (tag, errors) = self.encode_nbt(false);
        for error in errors {
            log::warn!("Item component serialization error: {error}");
        }
        tag
    }
}

fn hash_entry(key_hash: i32, value_hash: i32) -> HashEntry {
    let key_hash = key_hash as u32;
    let value_hash = value_hash as u32;
    HashEntry {
        key_hash: i64::from(key_hash),
        value_hash: i64::from(value_hash),
        key_bytes: key_hash.to_le_bytes(),
        value_bytes: value_hash.to_le_bytes(),
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
            entry.write_network(data, &mut buf)?;
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

            let component_data = entry.read_network(data).map_err(|e| {
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
            let component_data = entry.read_network(&mut sub_cursor)?;
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

impl EmbeddedNbtCodec for &DataComponentPatch {
    type Error = std::io::Error;

    fn encode_embedded_nbt(self) -> Result<OwnedNbtTag> {
        self.try_to_nbt_tag_ref()
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
                let component_data = entry.read_nbt(value)?;
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
pub fn component_try_into<T: Component + DowncastType>(
    data: &ComponentData,
    _component: DataComponentType<T>,
) -> Option<&T> {
    data.downcast_ref::<T>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        REGISTRY, RegistryExt as _,
        data_components::CustomData,
        data_components::vanilla_components::{
            ADDITIONAL_TRADE_COST, BREAK_SOUND, BUCKET_ENTITY_DATA, CHICKEN_VARIANT,
            CREATIVE_SLOT_LOCK, CUSTOM_NAME, DYE, ENCHANTABLE, ENCHANTMENT_GLINT_OVERRIDE,
            ITEM_MODEL, ITEM_NAME, LORE, MAP_COLOR, MAP_POST_PROCESSING, MAX_STACK_SIZE,
            OMINOUS_BOTTLE_AMPLIFIER, POTION_DURATION_SCALE, RARITY, STORED_ENCHANTMENTS,
            SWING_ANIMATION, SwingAnimationType, TOOLTIP_DISPLAY, USE_EFFECTS,
        },
        item_stack::ItemStack,
        sound_events,
        test_support::init_test_registry,
        vanilla_chicken_variants, vanilla_items,
    };
    use simdnbt::borrow::{NbtTag as BorrowedNbtTag, read_tag};
    use steel_utils::Identifier;
    use text_components::content::Content;

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
    fn duplicate_component_registration_is_rejected_without_mutation() {
        let mut registry = DataComponentRegistry::new();
        let key = Identifier::new("test".to_owned(), "duplicate".to_owned());
        let original = DataComponentType::<i32>::new(key.clone());
        registry.register(original.clone());

        let duplicate = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            registry.register(DataComponentType::<bool>::new(key.clone()));
        }));

        assert!(duplicate.is_err());
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.get_id(original), Some(0));
        assert_eq!(registry.get_key_by_id(0), Some(&key));
        assert_eq!(registry.by_key(&key).map(|entry| &entry.key), Some(&key));

        let second = DataComponentType::<i32>::new(Identifier::new(
            "test".to_owned(),
            "same_type_different_key".to_owned(),
        ));
        registry.register(second.clone());
        assert_eq!(registry.get_id(second), Some(1));
    }

    #[test]
    fn persistent_hash_rejects_values_rejected_by_the_persistent_codec() {
        let mut registry = DataComponentRegistry::new();
        super::super::vanilla_components::register_vanilla_data_components(&mut registry);

        let invalid_values = [
            (MAX_STACK_SIZE.key().clone(), ComponentData::new(0_i32)),
            (
                super::super::vanilla_components::MAX_DAMAGE.key().clone(),
                ComponentData::new(0_i32),
            ),
            (
                super::super::vanilla_components::MINIMUM_ATTACK_CHARGE
                    .key()
                    .clone(),
                ComponentData::new(1.5_f32),
            ),
            (
                POTION_DURATION_SCALE.key().clone(),
                ComponentData::new(-0.5_f32),
            ),
        ];
        for (key, value) in invalid_values {
            let entry = registry
                .by_key(&key)
                .expect("component should be registered");
            assert!(entry.compute_hash(&value).is_err(), "{key}");
        }

        let max_stack_size = registry
            .by_key(MAX_STACK_SIZE.key())
            .expect("max_stack_size should be registered");
        assert_eq!(
            max_stack_size
                .compute_hash(&ComponentData::new(99_i32))
                .expect("boundary value should hash"),
            99_i32.compute_hash()
        );
    }

    #[test]
    fn persistent_patch_nbt_omits_transient_components() {
        init_test_registry();
        let mut patch = DataComponentPatch::new();
        patch.set(MAX_STACK_SIZE, 16);
        patch.set(CREATIVE_SLOT_LOCK, ());
        patch.remove(ADDITIONAL_TRADE_COST);
        patch.remove(MAP_POST_PROCESSING);

        let OwnedNbtTag::Compound(compound) = patch.to_nbt_tag_ref() else {
            panic!("component patch should serialize as a compound");
        };
        assert!(compound.get("minecraft:max_stack_size").is_some());
        assert!(compound.get("minecraft:creative_slot_lock").is_none());
        assert!(compound.get("!minecraft:additional_trade_cost").is_none());
        assert!(compound.get("minecraft:map_post_processing").is_none());
    }

    #[test]
    fn persistent_patch_hash_uses_each_component_codec_hash() {
        init_test_registry();
        let mut patch = DataComponentPatch::new();
        patch.set(ENCHANTMENT_GLINT_OVERRIDE, true);

        let entry = hash_entry(
            ENCHANTMENT_GLINT_OVERRIDE.key.to_string().compute_hash(),
            true.compute_hash(),
        );
        let mut expected = ComponentHasher::new();
        expected.start_map();
        expected.put_raw_bytes(&entry.key_bytes);
        expected.put_raw_bytes(&entry.value_bytes);
        expected.end_map();
        assert_eq!(
            patch
                .compute_persistent_hash()
                .expect("valid patch should hash"),
            expected.finish()
        );

        // NbtOps stores Codec.BOOL as a byte while HashOps preserves a boolean.
        assert_ne!(
            patch
                .compute_persistent_hash()
                .expect("valid patch should hash"),
            patch.to_nbt_tag_ref().compute_hash()
        );
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
            Some(&ComponentPatchEntry::Set(ComponentData::new(16_i32)))
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

    #[test]
    fn text_component_persistent_codec_collapses_plain_text() {
        init_test_registry();
        let entry = REGISTRY
            .data_components
            .by_key(&CUSTOM_NAME.key)
            .expect("custom_name should be registered");
        let value = ComponentData::new(text_components::TextComponent::plain("name"));

        assert_eq!(
            entry
                .write_nbt(&value)
                .expect("plain custom name should encode"),
            OwnedNbtTag::String("name".into())
        );
    }

    #[test]
    fn common_defaults_and_extracted_item_overrides_match_vanilla() {
        init_test_registry();

        let common = DataComponentMap::common_item_components();
        assert_eq!(common.len(), 10);
        assert_eq!(common.get_ref(LORE), Some(&ItemLore::empty()));
        assert_eq!(common.get_ref(USE_EFFECTS), Some(&UseEffects::DEFAULT));
        assert_eq!(common.get_ref(RARITY), Some(&Rarity::Common));
        assert_eq!(
            common
                .get_ref(BREAK_SOUND)
                .and_then(SoundEventHolder::registry_ref),
            Some(&sound_events::ENTITY_ITEM_BREAK)
        );
        assert_eq!(
            common.get_ref(TOOLTIP_DISPLAY),
            Some(&TooltipDisplay::DEFAULT)
        );
        assert_eq!(
            common.get_ref(SWING_ANIMATION),
            Some(&SwingAnimation::DEFAULT)
        );

        let wooden_spear = ItemStack::new(&vanilla_items::WOODEN_SPEAR);
        assert_eq!(
            wooden_spear.get(USE_EFFECTS),
            Some(&UseEffects::new(true, false, 1.0))
        );
        assert_eq!(
            wooden_spear.get(SWING_ANIMATION),
            Some(&SwingAnimation::new(SwingAnimationType::Stab, 13))
        );

        let heavy_core = ItemStack::new(&vanilla_items::HEAVY_CORE);
        assert_eq!(heavy_core.get(RARITY), Some(&Rarity::Epic));

        let stone = ItemStack::new(&vanilla_items::STONE);
        assert_eq!(
            stone.get(ITEM_MODEL),
            Some(&Identifier::vanilla_static("stone"))
        );
        let Some(Content::Translate(stone_name)) = stone.get(ITEM_NAME).map(|name| &name.content)
        else {
            panic!("stone should have a translated item name");
        };
        assert_eq!(stone_name.key, "block.minecraft.stone");

        let redstone = ItemStack::new(&vanilla_items::REDSTONE);
        assert_eq!(
            redstone.get(ITEM_MODEL),
            Some(&Identifier::vanilla_static("redstone"))
        );
        let Some(Content::Translate(redstone_name)) =
            redstone.get(ITEM_NAME).map(|name| &name.content)
        else {
            panic!("redstone should have a translated item name");
        };
        assert_eq!(redstone_name.key, "item.minecraft.redstone");

        let shield = ItemStack::new(&vanilla_items::SHIELD);
        assert_eq!(
            shield
                .get(BREAK_SOUND)
                .and_then(SoundEventHolder::registry_ref),
            Some(&sound_events::ITEM_SHIELD_BREAK)
        );

        let pufferfish_bucket = ItemStack::new(&vanilla_items::PUFFERFISH_BUCKET);
        assert!(
            pufferfish_bucket
                .get(BUCKET_ENTITY_DATA)
                .is_some_and(CustomData::is_empty)
        );

        let golden_sword = ItemStack::new(&vanilla_items::GOLDEN_SWORD);
        assert_eq!(
            golden_sword.get(ENCHANTABLE).map(|value| value.value()),
            Some(22)
        );
        assert!(golden_sword.is_enchantable());
        assert!(!ItemStack::new(&vanilla_items::STONE).is_enchantable());

        for (item, variant) in [
            (&vanilla_items::EGG, &vanilla_chicken_variants::TEMPERATE),
            (&vanilla_items::BLUE_EGG, &vanilla_chicken_variants::COLD),
            (&vanilla_items::BROWN_EGG, &vanilla_chicken_variants::WARM),
        ] {
            assert!(
                ItemStack::new(item)
                    .get(CHICKEN_VARIANT)
                    .is_some_and(|reference| reference.value().key == variant.key),
                "{}",
                item.key
            );
        }

        assert_eq!(
            ItemStack::new(&vanilla_items::TIPPED_ARROW).get(POTION_DURATION_SCALE),
            Some(&0.125)
        );
        assert_eq!(
            ItemStack::new(&vanilla_items::LINGERING_POTION).get(POTION_DURATION_SCALE),
            Some(&0.25)
        );
        assert!(
            ItemStack::new(&vanilla_items::ENCHANTED_BOOK)
                .get(STORED_ENCHANTMENTS)
                .is_some_and(ItemEnchantments::is_empty)
        );

        let music_disc_cat = ItemStack::new(&vanilla_items::MUSIC_DISC_CAT);
        assert_eq!(
            music_disc_cat
                .get(crate::data_components::vanilla_components::JUKEBOX_PLAYABLE)
                .and_then(|playable| playable.song().as_reference()),
            Some(&crate::vanilla_jukebox_songs::CAT)
        );

        for (item, color) in [
            (&vanilla_items::WHITE_DYE, crate::DyeColor::White),
            (&vanilla_items::ORANGE_DYE, crate::DyeColor::Orange),
            (&vanilla_items::MAGENTA_DYE, crate::DyeColor::Magenta),
            (&vanilla_items::LIGHT_BLUE_DYE, crate::DyeColor::LightBlue),
            (&vanilla_items::YELLOW_DYE, crate::DyeColor::Yellow),
            (&vanilla_items::LIME_DYE, crate::DyeColor::Lime),
            (&vanilla_items::PINK_DYE, crate::DyeColor::Pink),
            (&vanilla_items::GRAY_DYE, crate::DyeColor::Gray),
            (&vanilla_items::LIGHT_GRAY_DYE, crate::DyeColor::LightGray),
            (&vanilla_items::CYAN_DYE, crate::DyeColor::Cyan),
            (&vanilla_items::PURPLE_DYE, crate::DyeColor::Purple),
            (&vanilla_items::BLUE_DYE, crate::DyeColor::Blue),
            (&vanilla_items::BROWN_DYE, crate::DyeColor::Brown),
            (&vanilla_items::GREEN_DYE, crate::DyeColor::Green),
            (&vanilla_items::RED_DYE, crate::DyeColor::Red),
            (&vanilla_items::BLACK_DYE, crate::DyeColor::Black),
        ] {
            assert_eq!(ItemStack::new(item).get(DYE), Some(&color), "{}", item.key);
        }

        assert_eq!(
            ItemStack::new(&vanilla_items::FILLED_MAP)
                .get(MAP_COLOR)
                .map(|color| color.rgb()),
            Some(4_603_950)
        );
        assert_eq!(
            ItemStack::new(&vanilla_items::OMINOUS_BOTTLE)
                .get(OMINOUS_BOTTLE_AMPLIFIER)
                .map(|amplifier| amplifier.value()),
            Some(0)
        );
    }
}
