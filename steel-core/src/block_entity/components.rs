//! Item-component application for block entities.
//!
//! Components read into entity fields are kept separate from the remaining
//! item component patch.

use rustc_hash::FxHashSet;
use steel_registry::data_components::{
    Component, ComponentPatchEntry, DataComponentMap, DataComponentType,
    vanilla_components::{BLOCK_ENTITY_DATA, BLOCK_STATE},
};
use steel_registry::item_stack::ItemStack;
use steel_utils::{DowncastType, Identifier};

/// Reads effective item components and tracks which ones are represented by
/// entity fields, excluding them from the stored component map.
pub struct ImplicitComponentGetter<'a> {
    stack: &'a ItemStack,
    consumed: FxHashSet<Identifier>,
}

impl<'a> ImplicitComponentGetter<'a> {
    pub(super) fn new(stack: &'a ItemStack) -> Self {
        let consumed = [BLOCK_ENTITY_DATA.key().clone(), BLOCK_STATE.key().clone()]
            .into_iter()
            .collect();
        Self { stack, consumed }
    }

    /// Reads a component from the placed stack and marks it implicit.
    pub fn get<T: Component + DowncastType + Clone>(
        &mut self,
        component: DataComponentType<T>,
    ) -> Option<T> {
        self.consumed.insert(component.key().clone());
        self.stack.get(component).cloned()
    }

    /// Reads a component from the placed stack, falling back to `default`, and marks it implicit.
    pub fn get_or_default<T: Component + DowncastType + Clone>(
        &mut self,
        component: DataComponentType<T>,
        default: T,
    ) -> T {
        self.get(component).unwrap_or(default)
    }

    /// Returns patched components that were set and not read into entity fields.
    pub(super) fn remaining_stored_components(self) -> DataComponentMap {
        let mut components = DataComponentMap::new();
        for (key, entry) in self.stack.components_patch().iter() {
            if self.consumed.contains(key) {
                continue;
            }
            if let ComponentPatchEntry::Set(data) = entry {
                components.set_raw(key.clone(), data.clone());
            }
        }
        components
    }
}
