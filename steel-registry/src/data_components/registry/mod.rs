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

mod codecs;
mod component_map;
mod patch;
mod patch_network;
mod patch_persistence;

pub(crate) use codecs::ValidatePersistentComponent;
pub use codecs::{
    ComponentEntry, ComponentEntryRef, NbtReader, NbtWriter, NetworkReader, NetworkWriter,
};
pub use component_map::DataComponentMap;
pub use patch::{ComponentPatchEntry, DataComponentPatch};
pub use patch_persistence::component_try_into;

use codecs::{
    PersistentCodecFns, hash_component, read_typed_nbt, read_typed_network, validate_component,
    write_typed_nbt, write_typed_network,
};

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

#[cfg(test)]
mod tests;
