//! Item identity, count, and effective components shared by stacks and templates.

use steel_utils::Identifier;

use crate::ItemStackTemplate;
use crate::data_components::{ComponentData, DataComponentGetter};
use crate::item_stack::ItemStack;
use crate::items::ItemRef;

pub trait ItemInstance: DataComponentGetter {
    fn item_ref(&self) -> ItemRef;
    fn item_count(&self) -> i32;
}

impl DataComponentGetter for ItemStack {
    fn effective_value_raw(&self, key: &Identifier) -> Option<&ComponentData> {
        self.get_effective_value_raw(key)
    }
}

impl ItemInstance for ItemStack {
    fn item_ref(&self) -> ItemRef {
        self.item()
    }

    fn item_count(&self) -> i32 {
        self.count()
    }
}

impl DataComponentGetter for ItemStackTemplate {
    fn effective_value_raw(&self, key: &Identifier) -> Option<&ComponentData> {
        self.get_effective_value_raw(key)
    }
}

impl ItemInstance for ItemStackTemplate {
    fn item_ref(&self) -> ItemRef {
        self.item()
    }

    fn item_count(&self) -> i32 {
        self.count()
    }
}
