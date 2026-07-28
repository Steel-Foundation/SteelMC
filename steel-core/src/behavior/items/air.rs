use std::borrow::Cow;

use steel_macros::item_behavior;
use steel_registry::{data_components::vanilla_components::ITEM_NAME, item_stack::ItemStack};
use text_components::TextComponent;

use crate::behavior::ItemBehavior;

/// Air behavior using the item type's prototype name like Vanilla `AirItem`.
#[item_behavior]
pub struct AirItem;

impl ItemBehavior for AirItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        stack
            .item()
            .components
            .get_ref(ITEM_NAME)
            .map_or_else(|| Cow::Owned(TextComponent::new()), Cow::Borrowed)
    }
}
