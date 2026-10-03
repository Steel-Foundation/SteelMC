use std::borrow::Cow;

use steel_macros::item_behavior;
use steel_registry::data_components::PotionContents;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_potions;
use text_components::TextComponent;

use crate::behavior::ItemBehavior;

use super::dynamic_name::potion_name;

/// Tipped-arrow behavior providing Vanilla's potion-content-dependent name and
/// poison default instance.
// TODO: Implement inherited ArrowItem projectile and dispenser behavior once
// ProjectileItem dispatch exists.
#[item_behavior]
pub struct TippedArrowItem;

impl ItemBehavior for TippedArrowItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        potion_name(stack)
    }

    fn get_default_instance(&self, item: ItemRef) -> ItemStack {
        PotionContents::create_item_stack(item, &vanilla_potions::POISON)
    }
}
