use std::borrow::Cow;

use steel_macros::item_behavior;
use steel_registry::{
    data_components::vanilla_components::LODESTONE_TRACKER, item_stack::ItemStack,
};
use text_components::TextComponent;

use crate::behavior::ItemBehavior;

use super::dynamic_name::{default_name, translated};

/// Compass behavior providing the lodestone-specific name.
// TODO: Implement lodestone binding and tracked-target invalidation when item
// inventory ticks are available.
#[item_behavior]
pub struct CompassItem;

impl ItemBehavior for CompassItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        if stack.has(LODESTONE_TRACKER) {
            translated("item.minecraft.lodestone_compass".to_owned(), None)
        } else {
            default_name(stack)
        }
    }
}
