use crate::stat::Stat;
use crate::{RegistryEntry, RegistryExt};
use rustc_hash::FxHashMap;
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use std::sync::LazyLock;
use steel_utils::{Downcast, DowncastType, DowncastTypeKey, ErasedType, Identifier};
use text_components::TextComponent;

/// Behavior required for a registry so that the values stored in that registry
/// can be used for identifying a particular stat.
pub trait StatValueRegistry: ErasedType + Send + Sync + 'static {
    fn len(&self) -> usize;
    fn key_from_id(&self, id: usize) -> Option<&'static Identifier>;
    fn value_from_id(&self, id: usize) -> Option<&'static dyn StatValueRegistryEntry>;
}

impl<R> StatValueRegistry for R
where
    R: RegistryExt + ErasedType + Send + Sync + 'static,
    R::Entry: StatValueRegistryEntry,
{
    fn len(&self) -> usize {
        self.len()
    }

    fn key_from_id(&self, id: usize) -> Option<&'static Identifier> {
        self.by_id(id).map(StatValueRegistryEntry::stat_value_key)
    }

    fn value_from_id(&self, id: usize) -> Option<&'static dyn StatValueRegistryEntry> {
        self.by_id(id)
            .map(|value| value as &dyn StatValueRegistryEntry)
    }
}

/// Behavior required for a registry entry so that it can be used for identifying a particular stat.
pub trait StatValueRegistryEntry: Send + Sync + 'static {
    // The functions here are prefixed so that it doesn't conflict
    // with those of RegistryEntry.
    fn stat_value_key(&self) -> &Identifier;
    fn stat_value_id(&self) -> usize;
}

impl<E> StatValueRegistryEntry for E
where
    E: RegistryEntry + Send + Sync,
{
    fn stat_value_key(&self) -> &Identifier {
        self.key()
    }

    fn stat_value_id(&self) -> usize {
        self.id()
    }
}

/// A structure that identifies a type of stat, using the
/// registry type [`R`] for using items from it to identify a particular stat.
pub struct StatType<R: RegistryExt> {
    /// The identifier that identifies this stat type uniquely.
    pub key: Identifier,
    pub display_name: Option<TextComponent>,

    _phantom: PhantomData<R>,
}

impl<R: RegistryExt> StatType<R>
where
    R::Entry: StatValueRegistryEntry,
{
    /// Creates a new [`StatType`] from a key and its display name.
    pub(crate) const fn new(key: Identifier, display_name: Option<TextComponent>) -> Self {
        Self {
            key,
            display_name,
            _phantom: PhantomData,
        }
    }

    /// Returns the identifying key of this stat type.
    #[must_use]
    pub const fn key(&self) -> &Identifier {
        &self.key
    }

    /// Returns the display name of this stat type.
    #[must_use]
    pub const fn display_name(&self) -> Option<&TextComponent> {
        self.display_name.as_ref()
    }

    /// Gets a [`Stat`] of this type with a given value.
    ///
    /// # Panics
    ///
    /// Panics if this stat type is unregistered with the [`StatTypeRegistry`].
    pub fn get(self, value: &'static R::Entry) -> Stat {
        Stat::new(self, value)
    }
}

/// A type-erased registry whose values can be used for identifying a particular stat.
/// Internally, the registry is stored in a [`LazyLock`] so that the reference
/// to the registry is only loaded after it has initialized.
///
/// Registries retain their concrete Rust type and can be recovered with [`Self::downcast_ref`].
#[derive(Copy, Clone)]
pub struct StatValueRegistryData {
    value: &'static dyn StatValueRegistry,
}

impl StatValueRegistryData {
    /// Erases the type of the provided registry.
    #[must_use]
    pub fn new(value: &'static dyn StatValueRegistry) -> Self {
        Self { value }
    }

    /// Returns the concrete registry when it has type `R`.
    #[must_use]
    pub fn downcast_ref<R: StatValueRegistry + DowncastType>(&self) -> Option<&'static R> {
        (*self.value).downcast_ref::<R>()
    }

    /// Returns the concrete type key of the registry involved in the data.
    #[must_use]
    pub fn type_key(&self) -> DowncastTypeKey {
        self.value.downcast_type_key()
    }
}

/// An entry stored in the stat type registry. It contains the registry responsible
/// for the encoding and decoding of the stat identity involved.
pub struct StatTypeEntry {
    /// The identifier of this stat type.
    pub key: Identifier,

    /// The display name of this stat type.
    pub display_name: Option<TextComponent>,

    /// The registry that can encode and decode the stat identity involved in this stat type.
    registry:
        LazyLock<StatValueRegistryData, Box<dyn FnOnce() -> StatValueRegistryData + Send + Sync>>,
}

impl StatTypeEntry {
    /// Gets the number of entries in the registry that this stat type is associated with.
    pub fn registry_len(&self) -> usize {
        self.registry.value.len()
    }

    /// Gets the key of an item by its registry ID.
    pub fn key_from_id(&self, id: usize) -> Option<&Identifier> {
        self.registry.value.key_from_id(id)
    }

    /// Gets the erased value of an item by its registry ID.
    pub fn value_from_id(&self, id: usize) -> Option<&dyn StatValueRegistryEntry> {
        self.registry.value.value_from_id(id)
    }
}

impl Debug for StatTypeEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("StatTypeEntry").field(&self.key).finish()
    }
}

pub type StatTypeEntryRef = &'static StatTypeEntry;

/// Registry of all stat types.
pub struct StatTypeRegistry {
    /// Stat types indexed by network ID.
    stat_types_by_id: Vec<StatTypeEntryRef>,
    /// Map which maps from the stat type identifier to its network ID.
    stat_types_by_key: FxHashMap<Identifier, usize>,
    /// Whether registration is still allowed.
    allows_registering: bool,
}

impl StatTypeRegistry {
    /// Creates a new registry for stat types.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stat_types_by_id: Vec::new(),
            stat_types_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers a stat type in this registry.
    ///
    /// The registry supplied in this function must be in a supplier so that
    /// it only runs once when the registries are initialized.
    pub fn register<R, F>(&mut self, stat_type: StatType<R>, registry_supplier: F)
    where
        R: RegistryExt + StatValueRegistry,
        F: (FnOnce() -> &'static R) + Send + Sync + 'static,
    {
        assert!(
            self.allows_registering,
            "Cannot register stat types after the registry has been frozen"
        );

        let entry = StatTypeEntry {
            key: stat_type.key.clone(),
            display_name: stat_type.display_name.clone(),
            registry: LazyLock::new(Box::new(|| StatValueRegistryData::new(registry_supplier()))),
        };

        let entry_ref = Box::leak(Box::new(entry));
        let id = self.stat_types_by_id.len();

        self.stat_types_by_id.push(entry_ref);
        self.stat_types_by_key.insert(stat_type.key.clone(), id);
    }

    /// Iterates all stat type entries in this registry.
    pub fn iter(&self) -> impl Iterator<Item = (usize, StatTypeEntryRef)> + '_ {
        self.stat_types_by_id
            .iter()
            .enumerate()
            .map(|(id, &entry)| (id, entry))
    }

    /// Gets the erased stat type entry from its statically typed stat type.
    #[must_use]
    pub fn by_stat_type<R: RegistryExt>(&self, stat_type: StatType<R>) -> Option<StatTypeEntryRef> {
        Some(self.stat_types_by_id[*self.stat_types_by_key.get(&stat_type.key)?])
    }
}

impl Default for StatTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    StatTypeRegistry,
    StatTypeEntry,
    stat_types_by_id,
    stat_types_by_key,
    stat_types
);
