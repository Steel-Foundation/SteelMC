use std::borrow::Cow;

use steel_macros::item_behavior;
use steel_registry::{
    data_components::vanilla_components::LODESTONE_TRACKER, item_stack::ItemStack,
};
use steel_utils::translations;
use text_components::TextComponent;

use crate::behavior::ItemBehavior;

use super::dynamic_name::default_name;

/// Compass behavior providing the lodestone-specific name.
// TODO: Implement lodestone binding and tracked-target invalidation when item
// inventory ticks are available.
#[item_behavior]
pub struct CompassItem;

impl ItemBehavior for CompassItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        if stack.has(LODESTONE_TRACKER) {
            Cow::Owned(
                translations::ITEM_MINECRAFT_LODESTONE_COMPASS
                    .msg()
                    .component(),
            )
        } else {
            default_name(stack)
        }
    }
}
