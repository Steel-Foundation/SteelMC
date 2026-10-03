use crate::stat::Stat;
use crate::{RegistryEntry, RegistryExt};
use rustc_hash::FxHashMap;
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use std::sync::{LazyLock, OnceLock};
use steel_utils::{Downcast, DowncastType, DowncastTypeKey, ErasedType, Identifier};

/// Behavior required for a registry so that the values stored in that registry
/// can be used for identifying a particular stat.
#[expect(clippy::len_without_is_empty)]
pub trait StatValueRegistry: ErasedType + Send + Sync + 'static {
    fn len(&self) -> usize;
    fn value_from_id(&self, id: usize) -> Option<&'static dyn StatValueRegistryEntry>;
    fn value_from_key(&self, key: &Identifier) -> Option<&'static dyn StatValueRegistryEntry>;

    fn key_from_id(&self, id: usize) -> Option<&'static Identifier> {
        self.value_from_id(id)
            .map(StatValueRegistryEntry::stat_value_key)
    }
    fn id_from_key(&self, key: &Identifier) -> Option<usize> {
        self.value_from_key(key)
            .map(StatValueRegistryEntry::stat_value_id)
    }
}

impl<R> StatValueRegistry for R
where
    R: RegistryExt + ErasedType + Send + Sync + 'static,
    R::Entry: StatValueRegistryEntry,
{
    fn len(&self) -> usize {
        self.len()
    }

    fn value_from_id(&self, id: usize) -> Option<&'static dyn StatValueRegistryEntry> {
        self.by_id(id)
            .map(|value| value as &dyn StatValueRegistryEntry)
    }

    fn value_from_key(&self, key: &Identifier) -> Option<&'static dyn StatValueRegistryEntry> {
        self.by_key(key)
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

    stat_type_entry_ref: OnceLock<StatTypeEntryRef>,
    _phantom: PhantomData<R>,
}

impl<R: RegistryExt> StatType<R>
where
    R::Entry: StatValueRegistryEntry,
{
    /// Creates a new [`StatType`] from a key and its display name.
    pub(crate) const fn new(key: Identifier) -> Self {
        Self {
            key,
            stat_type_entry_ref: OnceLock::new(),
            _phantom: PhantomData,
        }
    }

    /// Returns the identifying key of this stat type.
    #[must_use]
    pub const fn key(&self) -> &Identifier {
        &self.key
    }

    /// Gets the reference to the entry corresponding to their stat type.
    ///
    /// # Panics
    ///
    /// Panics if this stat type has not been registered with the [`StatTypeRegistry`].
    pub fn stat_type_entry_ref(&self) -> StatTypeEntryRef {
        self.stat_type_entry_ref
            .get()
            .expect("attempted to get the entry reference of an unregistered stat type")
    }

    /// Gets a [`Stat`] of this type with a given value.
    ///
    /// # Panics
    ///
    /// Panics if this stat type is unregistered with the [`StatTypeRegistry`].
    pub fn get(&'static self, value: &'static R::Entry) -> Stat {
        Stat::new(self, value)
    }
}

pub type StatTypeRef<R> = &'static StatType<R>;

/// A type-erased registry whose values can be used for identifying a particular stat.
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

/// An entry stored in the stat type registry. It represents a stat type.
///
/// A stat type is always associated with a registry whose values will be used to identify
/// stats from this stat type. Therefore, `StatTypeEntry` contains the registry responsible for the
/// encoding and decoding of the values involved.
///
/// For example, `ITEM_DROPPED` uses the item registry to have each item in this registry become a stat
/// under this stat type (like `diamond`) to track how many of those items have been dropped by the player.
///
/// Internally, the registry is stored in a [`LazyLock`], so that the reference
/// to the registry is only loaded after it has initialized.
pub struct StatTypeEntry {
    /// The identifier of this stat type.
    pub key: Identifier,

    /// The registry that can encode and decode the stat identity involved in this stat type.
    registry:
        LazyLock<StatValueRegistryData, Box<dyn FnOnce() -> StatValueRegistryData + Send + Sync>>,
}

impl StatTypeEntry {
    /// Gets the number of entries in this registry that this stat type is associated with.
    pub fn registry_len(&self) -> usize {
        self.registry.value.len()
    }

    /// Gets the key of an item in this registry by its registry ID.
    pub fn key_from_id(&self, id: usize) -> Option<&Identifier> {
        self.registry.value.key_from_id(id)
    }

    /// Gets the registry ID of an item in this registry by its key.
    pub fn id_from_key(&self, key: &Identifier) -> Option<usize> {
        self.registry.value.id_from_key(key)
    }

    /// Gets the erased value of an item in this registry by its registry ID.
    pub fn value_from_id(&self, id: usize) -> Option<&'static dyn StatValueRegistryEntry> {
        self.registry.value.value_from_id(id)
    }

    /// Gets the erased value of an item in this registry by its key.
    pub fn value_from_key(&self, key: &Identifier) -> Option<&'static dyn StatValueRegistryEntry> {
        self.registry.value.value_from_key(key)
    }
}

impl Debug for StatTypeEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("StatTypeEntry").field(&self.key).finish()
    }
}

pub type StatTypeEntryRef = &'static StatTypeEntry;

/// Registry of all stat types. A stat type is always associated with a registry whose values
/// will be used to identify stats from this stat type.
///
/// For example, `ITEM_DROPPED` uses the item registry to have each item in this registry become a stat
/// under this stat type (like `diamond`) to track how many of those items have been dropped by the player.
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
    pub fn register<R, F>(&mut self, stat_type: StatTypeRef<R>, registry_supplier: F)
    where
        R: RegistryExt + StatValueRegistry,
        F: (FnOnce() -> &'static R) + Send + Sync + 'static,
    {
        assert!(
            self.allows_registering,
            "Cannot register stat types after the registry has been frozen"
        );

        let key = &stat_type.key;
        assert!(
            !self.stat_types_by_key.contains_key(key),
            "Cannot register duplicate stat type key {key}"
        );

        let entry = StatTypeEntry {
            key: stat_type.key.clone(),
            registry: LazyLock::new(Box::new(|| StatValueRegistryData::new(registry_supplier()))),
        };

        let entry_ref = Box::leak(Box::new(entry));
        let id = self.stat_types_by_id.len();

        let locked_ref = stat_type.stat_type_entry_ref.get_or_init(|| entry_ref);
        assert_eq!(entry_ref, *locked_ref);

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
