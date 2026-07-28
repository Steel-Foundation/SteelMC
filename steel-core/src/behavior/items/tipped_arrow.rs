use std::borrow::Cow;

use steel_macros::item_behavior;
use steel_registry::item_stack::ItemStack;
use text_components::TextComponent;

use crate::behavior::ItemBehavior;

use super::dynamic_name::potion_name;

/// Tipped-arrow behavior providing Vanilla's potion-content-dependent name.
// TODO: Add TippedArrowItem's poison default instance when Steel has
// item-specific default-stack factories.
// TODO: Implement inherited ArrowItem projectile and dispenser behavior once
// ProjectileItem dispatch exists.
#[item_behavior]
pub struct TippedArrowItem;

impl ItemBehavior for TippedArrowItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        potion_name(stack)
    }
}
