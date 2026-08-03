use std::borrow::Cow;

use steel_macros::item_behavior;
use steel_registry::item_stack::ItemStack;
use text_components::TextComponent;

use crate::behavior::ItemBehavior;

use super::dynamic_name::potion_name;

/// Splash-potion behavior providing Vanilla's potion-content-dependent name.
// TODO: Implement inherited PotionItem.useOn water-to-mud conversion.
// TODO: Implement ThrowablePotionItem use and dispenser behavior once
// thrown-potion entities and ProjectileItem dispatch exist.
// TODO: Add the inherited water default instance when Steel has item-specific
// default-stack factories.
#[item_behavior]
pub struct SplashPotionItem;

impl ItemBehavior for SplashPotionItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        potion_name(stack)
    }
}

/// Lingering-potion behavior providing Vanilla's potion-content-dependent name.
// TODO: Implement inherited PotionItem.useOn water-to-mud conversion.
// TODO: Implement ThrowablePotionItem use and dispenser behavior once
// thrown-potion entities and ProjectileItem dispatch exist.
// TODO: Add the inherited water default instance when Steel has item-specific
// default-stack factories.
#[item_behavior]
pub struct LingeringPotionItem;

impl ItemBehavior for LingeringPotionItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        potion_name(stack)
    }
}
