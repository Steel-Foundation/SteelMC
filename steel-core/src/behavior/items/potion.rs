use std::borrow::Cow;

use steel_macros::item_behavior;
use steel_registry::item_stack::ItemStack;
use text_components::TextComponent;

use crate::behavior::ItemBehavior;

use super::dynamic_name::potion_name;

/// Potion behavior providing Vanilla's potion-content-dependent name.
// TODO: Implement PotionItem.useOn water-to-mud conversion, bottle replacement,
// sounds, particles, and FLUID_PLACE.
// TODO: Add PotionItem's water default instance when Steel has item-specific
// default-stack factories.
// TODO: Complete the shared CONSUMABLE use/finish lifecycle so potions can be
// drunk.
#[item_behavior]
pub struct PotionItem;

impl ItemBehavior for PotionItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        potion_name(stack)
    }
}
