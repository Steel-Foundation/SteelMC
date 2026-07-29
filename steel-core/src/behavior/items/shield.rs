use std::borrow::Cow;

use steel_macros::item_behavior;
use steel_registry::{data_components::vanilla_components::BASE_COLOR, item_stack::ItemStack};
use text_components::TextComponent;

use crate::behavior::ItemBehavior;

use super::dynamic_name::{default_name, description_id, translated};

/// Shield behavior providing the base-color-specific name.
// TODO: Complete the shared Item.use BLOCKS_ATTACKS path so shields can block.
#[item_behavior]
pub struct ShieldItem;

impl ItemBehavior for ShieldItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        let Some(color) = stack.get(BASE_COLOR) else {
            return default_name(stack);
        };
        let Some(description_id) = description_id(stack) else {
            return default_name(stack);
        };
        translated(
            format!("{description_id}.{}", color.serialized_name()),
            None,
        )
    }
}
