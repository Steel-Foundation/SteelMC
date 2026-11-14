//! Shared access to effective components without materializing an item stack.

use steel_utils::{DowncastType, Identifier};

use super::{Component, ComponentData, DataComponentType};

/// Object-safe access to effective component values, including item defaults.
pub trait DataComponentGetter {
    fn effective_value_raw(&self, key: &Identifier) -> Option<&ComponentData>;
}

/// Typed access for concrete and erased component getters.
pub trait DataComponentGetterExt: DataComponentGetter {
    fn component<T: Component + DowncastType>(
        &self,
        component: DataComponentType<T>,
    ) -> Option<&T> {
        self.effective_value_raw(component.key())
            .and_then(ComponentData::downcast_ref::<T>)
    }
}

impl<T: DataComponentGetter + ?Sized> DataComponentGetterExt for T {}
