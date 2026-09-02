use steel_utils::{DowncastType, Identifier};

use super::{Component, ComponentData, DataComponentType};

/// Read-only access to effective component values.
///
/// Mirrors Vanilla `DataComponentGetter`. Item stacks and templates resolve
/// values through their patch and item prototype; component maps expose their
/// stored values directly.
pub trait DataComponentGetter {
    /// Returns the effective raw value stored for `key`.
    fn get_raw(&self, key: &Identifier) -> Option<&ComponentData>;
}

impl dyn DataComponentGetter + '_ {
    /// Returns the effective typed value for `component`.
    #[must_use]
    pub fn get<T: Component + DowncastType>(&self, component: DataComponentType<T>) -> Option<&T> {
        self.get_raw(&component.key)
            .and_then(ComponentData::downcast_ref::<T>)
    }
}
