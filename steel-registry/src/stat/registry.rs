use crate::RegistryExt;
use rustc_hash::FxHashMap;
use std::marker::PhantomData;
use std::sync::LazyLock;
use steel_utils::{Downcast, DowncastType, DowncastTypeKey, ErasedType, Identifier};
use text_components::TextComponent;

/// A structure that identifies a type of stat.
///
/// As this handle is typed, it ensures type-safety when used
/// with values that this type identifies with.
pub struct StatType<T> {
    pub(crate) key: Identifier,
    pub(crate) display_name: TextComponent,

    _phantom: PhantomData<T>,
}

impl<T> StatType<T> {
    /// Creates a new [`StatType`] from a key and its display name.
    pub(crate) const fn new(key: Identifier, display_name: TextComponent) -> Self {
        Self {
            key,
            display_name,
            _phantom: PhantomData,
        }
    }
}

/// Behavior required for a registry so that the values stored in that registry
/// can be used as a stat key of a stat type.
pub trait StatKeyRegistry: ErasedType + Send + Sync + 'static {}

impl<E, R> StatKeyRegistry for R where R: RegistryExt<Entry = E> + ErasedType + Send + Sync + 'static {}

/// A type-erased registry whose values can be used as a stat key of a stat type.
/// Internally, the registry is stored in a [`LazyLock`] so that the reference
/// to the registry is only loaded after it has initialized.
///
/// Registries retain their concrete Rust type and can be recovered with [`Self::downcast_ref`].
pub struct StatKeyRegistryData {
    value: &'static dyn StatKeyRegistry,
}

impl StatKeyRegistryData {
    /// Erases a typed stat key registry value.
    #[must_use]
    pub fn new(value: &'static dyn StatKeyRegistry) -> Self {
        Self { value }
    }

    /// Returns the concrete registry when it has type `R`.
    #[must_use]
    pub fn downcast_ref<R: StatKeyRegistry + DowncastType>(&self) -> Option<&'static R> {
        (*self.value).downcast_ref::<R>()
    }

    /// Returns the concrete type key of the registry involved in the data.
    #[must_use]
    pub fn type_key(&self) -> DowncastTypeKey {
        self.value.downcast_type_key()
    }
}

/// An entry stored in the stat type registry. It contains the registry responsible
/// for the encoding and decoding of the stat key involved.
pub struct StatTypeEntry {
    /// The identifier of this stat type.
    pub key: Identifier,

    /// The display name of this stat type.
    pub display_name: TextComponent,

    /// The registry that can encode the stat key involved in this stat type.
    registry: LazyLock<StatKeyRegistryData, Box<dyn FnOnce() -> StatKeyRegistryData + Send + Sync>>,
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
    pub fn register<T, R, F>(&mut self, stat_type: StatType<T>, registry_supplier: F)
    where
        R: RegistryExt<Entry = T> + StatKeyRegistry,
        F: (FnOnce() -> &'static R) + Send + Sync + 'static,
    {
        assert!(
            self.allows_registering,
            "Cannot register stat types after the registry has been frozen"
        );

        let entry = StatTypeEntry {
            key: stat_type.key.clone(),
            display_name: stat_type.display_name.clone(),
            registry: LazyLock::new(Box::new(|| StatKeyRegistryData::new(registry_supplier()))),
        };

        let entry_ref = Box::leak(Box::new(entry));

        let id = self.stat_types_by_id.len();
        self.stat_types_by_id.push(entry_ref);
        self.stat_types_by_key.insert(stat_type.key.clone(), id);
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, StatTypeEntryRef)> + '_ {
        self.stat_types_by_id
            .iter()
            .enumerate()
            .map(|(id, &entry)| (id, entry))
    }
}

crate::impl_registry!(
    StatTypeRegistry,
    StatTypeEntry,
    stat_types_by_id,
    stat_types_by_key,
    stat_types
);
