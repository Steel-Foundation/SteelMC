use super::{
    Component, ComponentData, DataComponentMap, DataComponentType, Debug, DowncastType, FxHashMap,
    Identifier,
};

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
    pub(super) entries: FxHashMap<Identifier, ComponentPatchEntry>,
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
}
